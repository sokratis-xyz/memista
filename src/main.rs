use std::sync::Arc;
use std::collections::HashMap;
use actix_web::{web, App, HttpServer, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use anyhow::{Result};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind, new_index};
use std::sync::Mutex;
use async_sqlite::{Pool, PoolBuilder, JournalMode};
use apistos::{api_operation, ApiComponent};
use apistos::app::{BuildConfig, OpenApiWrapper};
use apistos::info::Info;
use apistos::server::Server;
use apistos::spec::Spec;
use apistos::web::{post, delete, resource, scope};
use apistos::{RapidocConfig, RedocConfig, ScalarConfig, SwaggerUIConfig};
use schemars::JsonSchema;

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct InsertChunkRequest {
    database_id: String,
    chunk_id: u64,
    url: String,
    content: String,
    embedding: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, ApiComponent)]
struct SearchRequest {
    database_id: String,
    query: String,
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
                chunk_id INTEGER PRIMARY KEY,
                url TEXT,
                content TEXT
            )", table_name),
            [],
        )
    }).await.map_err(actix_web::error::ErrorInternalServerError)?;
    Ok(())
}

#[api_operation(summary = "Insert a chunk into the database")]
async fn insert_chunk(
    app_state: web::Data<Arc<AppState>>,
    request: web::Json<InsertChunkRequest>,
) -> actix_web::Result<HttpResponse> {

    let index_file = format!("{}.usearch", request.database_id);

    let options = IndexOptions {
        dimensions: 100,
        metric: MetricKind::IP,
        quantization: ScalarKind::F32,
        connectivity: 0,
        expansion_add: 0,
        expansion_search: 0,
        multi: false,
    };
    let index: Index = new_index(&options).unwrap();
    
    index.load(&index_file).is_ok();

    ensure_table_exists(&app_state.db_pool, &request.database_id).await?;
    let table_name = format!("chunks_{}", request.database_id);
    app_state.db_pool.conn(move |conn| {
        conn.execute(
            &format!("INSERT OR REPLACE INTO {} (chunk_id, url, content) VALUES (?, ?, ?)", table_name),
            [&request.chunk_id.to_string(), &request.url, &request.content],
        )
    }).await.map_err(actix_web::error::ErrorInternalServerError)?;

    // This needs to be in a loop
    // index.add(42, &first).is_ok()
    // index.save(&index_file).is_ok()

    Ok(HttpResponse::Ok().json(json!({"status": "success"})))
}

#[api_operation(summary = "Search for chunks")]
async fn search(
    app_state: web::Data<Arc<AppState>>,
    request: web::Json<SearchRequest>,
) -> actix_web::Result<HttpResponse> {

    let index_file = format!("{}.usearch", request.database_id);

    let options = IndexOptions {
        dimensions: 100,
        metric: MetricKind::IP,
        quantization: ScalarKind::F32,
        connectivity: 0,
        expansion_add: 0,
        expansion_search: 0,
        multi: false,
    };
    let index: Index = new_index(&options).unwrap();
    
    index.load(&index_file).is_ok();

    ensure_table_exists(&app_state.db_pool, &request.database_id).await?;

    // TODO: Implement embedding generation for the query
    let query_embedding: Vec<f32> = vec![]; // This should be generated from the query text

    let results = index.search(&request.query, 10).unwrap();

    let table_name = format!("chunks_{}", request.database_id);
    let mut ranked_chunks = Vec::new();
    for (chunk_id, _score) in results {
        let chunk = app_state.db_pool.conn(|conn| {
            conn.query_row(
                &format!("SELECT url, content FROM {} WHERE chunk_id = ?", table_name),
                [&chunk_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
        }).await.map_err(actix_web::error::ErrorInternalServerError)?;

        ranked_chunks.push(json!({
            "chunk_id": chunk_id,
            "url": chunk.0,
            "content": chunk.1,
        }));
    }

    Ok(HttpResponse::Ok().json(ranked_chunks))
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
    std::fs::remove_file(index_file).map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().json(json!({"status": "success", "message": "Table dropped successfully"})))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db_pool = PoolBuilder::new()
        .path("memista.db")
        .journal_mode(JournalMode::Wal)
        .open()
        .await
        .expect("Failed to create database pool");

    let app_state = Arc::new(AppState {
        db_pool
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
    .bind("127.0.0.1:8080")?
    .run()
    .await
}