use std::sync::Arc;

use codanna::config::{SemanticSearchConfig, Settings};
use codanna::indexing::facade::IndexFacade;
use codanna::mcp::{
    CodeIntelligenceServer, FindSymbolRequest, SemanticSearchRequest,
    SemanticSearchWithContextRequest,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::RawContent;
use tempfile::TempDir;

const REPOSITORY_FIXTURE: &str = r#"
package com.opensea.data.currency

import com.opensea.common.components.postgres.core.client.ReadWritePgClient
import com.opensea.common.components.postgres.core.client.PgClient
import java.util.UUID

/**
 * Repository for managing currency data in Aurora database.
 * Handles currency collection operations with read/write client separation.
 */
class AuroraCurrencyRepository(
    private val readClient: PgClient,
    private val writeClient: ReadWritePgClient,
) {
    /**
     * Update currency collections for a given currency ID.
     * Uses write client for data modifications.
     */
    suspend fun updateCurrencyCollections(
        id: UUID,
        collectionId: UUID,
        clearCollections: Boolean = false,
    ): CurrencyModel? {
        return writeClient.execute(
            UPDATE_CURRENCY_COLLECTIONS,
            id,
            collectionId,
            clearCollections,
        )
    }

    /**
     * Fetch currency by ID using read client.
     */
    suspend fun getCurrency(id: UUID): CurrencyModel? {
        return readClient.query("SELECT * FROM currency WHERE id = ?", id)
    }
}
"#;

const CLIENT_FIXTURE: &str = r#"
package com.opensea.common.components.postgres.core.client

interface PgClient {
    fun query(sql: String, vararg params: Any): List<Row>
}

/**
 * Read-write PostgreSQL client with connection pooling.
 * Provides both read and write operations with automatic retry logic.
 */
class ReadWritePgClient : PgClient {
    override fun query(sql: String, vararg params: Any): List<Row> {
        return emptyList()
    }

    /**
     * Execute write operations with automatic retry on failure.
     */
    fun execute(sql: String, vararg params: Any): List<Row> {
        return emptyList()
    }
}
"#;

const SERVICE_FIXTURE: &str = r#"
package com.opensea.services

import com.opensea.data.currency.AuroraCurrencyRepository
import com.opensea.common.components.postgres.core.client.ReadWritePgClient

/**
 * Service for handling currency operations.
 * Uses repository pattern with dependency injection.
 */
class CurrencyService(
    private val repository: AuroraCurrencyRepository,
    private val dbClient: ReadWritePgClient,
) {
    /**
     * Process currency update request.
     */
    suspend fun updateCurrency(id: String, amount: Double) {
        repository.updateCurrencyCollections(
            UUID.fromString(id),
            UUID.randomUUID(),
        )
    }
}
"#;

#[tokio::test(flavor = "current_thread")]
#[ignore = "Downloads 86MB embedding model - unsuitable for CI/CD. Run with: cargo test test_kotlin_semantic -- --ignored"]
async fn test_kotlin_semantic_search_and_dependency_injection() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let workspace_root = temp_dir.path();

    let fixtures = [
        (
            "src/main/kotlin/data/currency/AuroraCurrencyRepository.kt",
            REPOSITORY_FIXTURE,
        ),
        (
            "src/main/kotlin/common/components/postgres/core/client/ReadWritePgClient.kt",
            CLIENT_FIXTURE,
        ),
        (
            "src/main/kotlin/services/CurrencyService.kt",
            SERVICE_FIXTURE,
        ),
    ];

    // Create Kotlin source files
    for (relative_path, contents) in fixtures {
        let full_path = workspace_root.join(relative_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture directory");
        }
        std::fs::write(&full_path, contents).expect("write fixture");
    }

    let index_path = workspace_root.join(".codanna-index");
    std::fs::create_dir_all(&index_path).expect("create index directory");

    let settings = Settings {
        workspace_root: Some(workspace_root.to_path_buf()),
        index_path: index_path.clone(),
        semantic_search: SemanticSearchConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let settings = Arc::new(settings);
    let mut indexer = IndexFacade::new(settings.clone()).expect("Failed to create IndexFacade");
    indexer
        .enable_semantic_search()
        .expect("enable semantic search");

    // Index all Kotlin files
    for (relative, _) in fixtures {
        let file_path = workspace_root.join(relative);
        indexer
            .index_file(file_path.to_str().expect("utf8 path"))
            .expect("index fixture file");
    }

    let server = CodeIntelligenceServer::new(indexer);

    // Test 1: Semantic search should find Kotlin classes
    let semantic_result = server
        .semantic_search_docs(Parameters(SemanticSearchRequest {
            query: "repository for currency database operations".to_string(),
            limit: 5,
            threshold: None,
            lang: Some("kotlin".to_string()),
        }))
        .await
        .expect("semantic_search_docs should succeed");

    let semantic_text = semantic_result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        semantic_text.contains("AuroraCurrencyRepository"),
        "expected semantic search to find AuroraCurrencyRepository, got:\n{semantic_text}"
    );

    // Test 2: Semantic search with context should include dependencies
    let context_result = server
        .semantic_search_with_context(Parameters(SemanticSearchWithContextRequest {
            query: "PostgreSQL client with write operations".to_string(),
            limit: 3,
            threshold: None,
            lang: Some("kotlin".to_string()),
        }))
        .await
        .expect("semantic_search_with_context should succeed");

    let context_text = context_result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        context_text.contains("ReadWritePgClient"),
        "expected semantic search to find ReadWritePgClient, got:\n{context_text}"
    );

    // Test 3: Find symbols by name should work for Kotlin classes
    let find_result = server
        .find_symbol(Parameters(FindSymbolRequest {
            name: "ReadWritePgClient".to_string(),
            lang: Some("kotlin".to_string()),
        }))
        .await
        .expect("find_symbol should succeed");

    let find_text = find_result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        find_text.contains("symbol_id:"),
        "expected find_symbol to return symbol with ID, got:\n{find_text}"
    );
    assert!(
        find_text.contains("Class"),
        "expected ReadWritePgClient to be indexed as Class, got:\n{find_text}"
    );

    // Test 4: Verify embeddings were created for Kotlin symbols
    let embedding_count = server
        .get_facade_arc()
        .read()
        .await
        .semantic_search_embedding_count();

    assert!(
        embedding_count >= 3,
        "expected at least 3 embeddings for Kotlin classes, got: {embedding_count}"
    );

    println!("\n✅ All Kotlin semantic search tests passed!");
    println!("   - Found AuroraCurrencyRepository via semantic search");
    println!("   - Found ReadWritePgClient via semantic search");
    println!("   - Verified symbol indexing for Kotlin classes");
    println!("   - Created {embedding_count} embeddings for Kotlin symbols");
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "Downloads 86MB embedding model - unsuitable for CI/CD. Run with: cargo test test_kotlin_semantic -- --ignored"]
async fn test_kotlin_dependency_injection_discovery() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let workspace_root = temp_dir.path();

    // Create a simpler fixture focused on DI
    let di_fixture = r#"
package com.example

/**
 * Database client for read/write operations.
 */
class DatabaseClient {
    fun query(sql: String): List<Any> = emptyList()
}

/**
 * User repository with injected database client.
 */
class UserRepository(
    private val dbClient: DatabaseClient,
) {
    fun findUser(id: Long) = dbClient.query("SELECT * FROM users WHERE id = $id")
}

/**
 * User service with multiple dependencies.
 */
class UserService(
    private val repository: UserRepository,
    private val client: DatabaseClient,
) {
    fun getUser(id: Long) = repository.findUser(id)
}
"#;

    let file_path = workspace_root.join("DependencyInjection.kt");
    std::fs::write(&file_path, di_fixture).expect("write fixture");

    let index_path = workspace_root.join(".codanna-index");
    std::fs::create_dir_all(&index_path).expect("create index directory");

    let settings = Settings {
        workspace_root: Some(workspace_root.to_path_buf()),
        index_path: index_path.clone(),
        semantic_search: SemanticSearchConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let settings = Arc::new(settings);
    let mut indexer = IndexFacade::new(settings.clone()).expect("Failed to create IndexFacade");
    indexer
        .enable_semantic_search()
        .expect("enable semantic search");

    indexer
        .index_file(file_path.to_str().expect("utf8 path"))
        .expect("index fixture file");

    let server = CodeIntelligenceServer::new(indexer);

    // Test: Semantic search for "classes that use DatabaseClient"
    let result = server
        .semantic_search_docs(Parameters(SemanticSearchRequest {
            query: "classes that inject database client dependency".to_string(),
            limit: 10,
            threshold: None,
            lang: Some("kotlin".to_string()),
        }))
        .await
        .expect("semantic_search_docs should succeed");

    let text = result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Should find classes that have DatabaseClient as constructor parameter
    let has_user_service = text.contains("UserService");
    let has_user_repository = text.contains("UserRepository");

    assert!(
        has_user_service || has_user_repository,
        "expected semantic search to find classes with DatabaseClient dependency, got:\n{text}"
    );

    println!("\n✅ Kotlin dependency injection discovery test passed!");
    println!("   - Semantic search found classes with injected dependencies");
}
