
# Memista : high-performance vector search service

Memista is a high-performance vector search service written in Rust that provides a simple HTTP API for storing and retrieving text chunks with their associated vector embeddings. It combines SQLite for metadata storage with USearch for efficient vector similarity search.

## Features

- Fast vector similarity search using USearch
- Persistent storage of text chunks and metadata in SQLite
- Multiple database support through database_id partitioning
- OpenAPI documentation with multiple UI options (Swagger, Redoc, RapiDoc)
- Configurable through environment variables
- Async I/O for high performance

## API Endpoints

### POST /v1/insert
Insert text chunks with their embeddings into a specified database.

### POST /v1/search
Search for similar chunks using vector embeddings.

### DELETE /v1/drop
Drop a specific database and its associated vector index.

## Configuration

The service can be configured using environment variables:

- `DATABASE_PATH`: Path to SQLite database file (default: "memista.db")
- `SERVER_HOST`: Host address to bind to (default: "127.0.0.1")
- `SERVER_PORT`: Port to listen on (default: 8083)
- `LOG_LEVEL`: Logging level (default: "info")

## Quick Start

1. Install Rust and cargo
2. Clone this repository
3. Create a `.env` file with your configuration (optional)
4. Run the server:

```bash
cargo run
```

The server will start and the API documentation will be available at:
- Swagger UI: http://localhost:8083/swagger
- Redoc: http://localhost:8083/redoc
- RapiDoc: http://localhost:8083/rapidoc
- OpenAPI JSON: http://localhost:8083/openapi.json

## Example Usage

### Insert Chunks

```bash
curl -X POST http://localhost:8083/v1/insert \
  -H "Content-Type: application/json" \
  -d '{
    "database_id": "my_db",
    "chunks": [{
      "embedding": [0.1, 0.2],
      "text": "Sample text",
      "metadata": "{\"source\": \"document1\"}"
    }]
  }'
```

### Search Chunks

```bash
curl -X POST http://localhost:8083/v1/search \
  -H "Content-Type: application/json" \
  -d '{
    "database_id": "my_db",
    "embeddings": [[0.1, 0.2]],
    "num_results": 5
  }'
```

## Dependencies

The project uses several key dependencies:
- actix-web: Web framework
- usearch: Vector similarity search
- async-sqlite: Async SQLite database interface
- apistos: OpenAPI documentation generation
- serde: Serialization/deserialization

For a complete list of dependencies, see the Cargo.toml file.

## 📝 License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0) - see below for a summary:

GNU General Public License v3.0 (GPL-3.0)

Permissions:
- Commercial use
- Distribution
- Modification
- Patent use
- Private use

Conditions:
- Disclose source
- License and copyright notice
- Same license
- State changes

Limitations:
- Liability
- Warranty

For the full license text, see [LICENSE](LICENSE) or visit https://www.gnu.org/licenses/gpl-3.0.en.html

## 📧 Contact

support@sokratis.xyz

