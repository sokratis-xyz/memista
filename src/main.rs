use std::sync::Arc;
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use anyhow::Result;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind, new_index};
use async_sqlite::{Pool, PoolBuilder, JournalMode};
use apistos::{api_operation, ApiComponent};
use apistos::app::{BuildConfig, OpenApiWrapper};
use apistos::info::Info;
use apistos::server::Server;
use apistos::spec::Spec;
use apistos::web::{post, delete, resource, scope};
use apistos::{RapidocConfig, RedocConfig, ScalarConfig, SwaggerUIConfig};
use schemars::JsonSchema;

use log::{debug, info, warn};


#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct ChunkData {
    embedding: Vec<f32>,
    text: String,
    metadata: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct InsertChunkRequest {
    database_id: String,
    chunks: Vec<ChunkData>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct SearchRequest {
    database_id: String,
    embeddings: Vec<Vec<f32>>,
    num_results: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct SearchResult {
    text: String,
    metadata: Option<String>,
    score: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct DropTableRequest {
    database_id: String,
}

struct AppState {
    db_pool: Pool,
}

async fn ensure_table_exists(db_pool: &Pool, database_id: &str) -> Result<(), actix_web::Error> {
    let table_name = format!("chunks_{}", database_id);
    db_pool.conn(move |conn| {
        conn.execute(
            &format!("CREATE TABLE IF NOT EXISTS {} (
                chunk_id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT,
                metadata TEXT
            )", table_name),
            [],
        )
    }).await.map_err(actix_web::error::ErrorInternalServerError)?;
    Ok(())
}

fn load_or_create_index(database_id: &str) -> Result<Index, actix_web::Error> {
    let index_file = format!("{}.usearch", database_id);
    let options = IndexOptions {
        dimensions: 2,
        metric: MetricKind::IP,
        quantization: ScalarKind::F32,
        connectivity: 0,
        expansion_add: 0,
        expansion_search: 0,
        multi: true,
    };
    let index: Index = new_index(&options).map_err(actix_web::error::ErrorInternalServerError)?;
    
    if std::path::Path::new(&index_file).exists() {
        index.load(&index_file).map_err(actix_web::error::ErrorInternalServerError)?;
    }
    
    Ok(index)
}

#[api_operation(summary = "Insert chunks into the database")]
async fn insert_chunk(
    app_state: web::Data<Arc<AppState>>,
    request: web::Json<InsertChunkRequest>,
) -> actix_web::Result<HttpResponse> {

    log::debug!("Loading index");

    let mut index = load_or_create_index(&request.database_id)?;

    index.reserve(request.chunks.len() + index.size()).map_err(actix_web::error::ErrorInternalServerError)?;

    log::debug!("Loaded index {}", &request.database_id);

    ensure_table_exists(&app_state.db_pool, &request.database_id).await?;

    log::debug!("Ensured table exists {}", &request.database_id);
    
    let table_name = format!("chunks_{}", request.database_id);

    let mut inserted_ids = Vec::new();

    for chunk in &request.chunks {
        let chunk = chunk.clone();
        let table_name = table_name.clone();

        log::debug!("inserting into database");
        let chunk_id: i64 = app_state.db_pool.conn(move |conn| {
            conn.query_row(
                &format!("INSERT INTO {} (text, metadata) VALUES (?, ?) RETURNING chunk_id", table_name),
                [&chunk.text, &chunk.metadata],
                |row| row.get(0),
            )
        }).await.map_err(actix_web::error::ErrorInternalServerError)?;
        
        log::debug!("inserting into vector index");

        index.add(chunk_id as u64, &chunk.embedding).map_err(actix_web::error::ErrorInternalServerError)?;

        inserted_ids.push(chunk_id);
    }

    let index_file = format!("{}.usearch", request.database_id);
    index.save(&index_file).map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().json(json!({ "inserted_ids": inserted_ids })))
}

#[api_operation(summary = "Search for chunks")]
async fn search(
    app_state: web::Data<Arc<AppState>>,
    request: web::Json<SearchRequest>,
) -> actix_web::Result<HttpResponse> {
    let index = load_or_create_index(&request.database_id)?;

    ensure_table_exists(&app_state.db_pool, &request.database_id).await?;
    let table_name = format!("chunks_{}", request.database_id);

    let mut all_results = Vec::new();

    for query_embedding in &request.embeddings {
        let results = index.search(query_embedding, request.num_results).map_err(actix_web::error::ErrorInternalServerError)?;
        
        let mut ranked_chunks = Vec::new();
        for (chunk_id, score) in results.keys.iter().zip(results.distances.iter()) {
            let chunk_id = *chunk_id;
            let score = *score;
            let table_name = table_name.clone();
            let chunk = app_state.db_pool.conn(move |conn| {
                conn.query_row(
                    &format!("SELECT text, metadata FROM {} WHERE chunk_id = ?", table_name),
                    [chunk_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
            }).await.map_err(actix_web::error::ErrorInternalServerError)?;

            ranked_chunks.push(SearchResult {
                text: chunk.0,
                metadata: chunk.1,
                score,
            });
        }

        all_results.push(ranked_chunks);
    }

    Ok(HttpResponse::Ok().json(all_results))
}

#[api_operation(summary = "Drop a table for a specific database")]
async fn drop_table(
    app_state: web::Data<Arc<AppState>>,
    request: web::Json<DropTableRequest>,
) -> actix_web::Result<HttpResponse> {
    let table_name = format!("chunks_{}", request.database_id);
    
    app_state.db_pool.conn(move |conn| {
        conn.execute(
            &format!("DROP TABLE IF EXISTS {}", table_name),
            [],
        )
    }).await.map_err(actix_web::error::ErrorInternalServerError)?;
    
    let index_file = format!("{}.usearch", request.database_id);
    if std::path::Path::new(&index_file).exists() {
        std::fs::remove_file(index_file).map_err(actix_web::error::ErrorInternalServerError)?;
    }

    Ok(HttpResponse::Ok().json(json!({"status": "success", "message": "Table and index dropped successfully"})))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let db_pool = PoolBuilder::new()
        .path("memista.db")
        .journal_mode(JournalMode::Wal)
        .open()
        .await
        .expect("Failed to create database pool");

    let app_state = Arc::new(AppState {
        db_pool,
    });

    HttpServer::new(move || {
        let spec = Spec {
            info: Info {
                title: "Vector Search API".to_string(),
                description: Some("Vector Search API for chunk storage and retrieval".to_string()),
                ..Default::default()
            },
            servers: vec![Server {
                url: "/".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .document(spec)
            .service(scope("/v1")
                .service(resource("/insert").route(post().to(insert_chunk)))
                .service(resource("/search").route(post().to(search)))
                .service(resource("/drop").route(delete().to(drop_table)))
            )
            .build_with(
                "/openapi.json",
                BuildConfig::default()
                    .with(RapidocConfig::new(&"/rapidoc"))
                    .with(RedocConfig::new(&"/redoc"))
                    .with(ScalarConfig::new(&"/scalar"))
                    .with(SwaggerUIConfig::new(&"/swagger")),
            )
    })
    .bind("127.0.0.1:8083")?
    .run()
    .await
}