use std::sync::Arc;

use codanna::SimpleIndexer;
use codanna::config::{SemanticSearchConfig, Settings};
use codanna::mcp::{
    AnalyzeImpactRequest, CodeIntelligenceServer, SemanticSearchWithContextRequest,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::RawContent;
use tempfile::TempDir;

const PLAYER_FIXTURE: &str = include_str!("../fixtures/gdscript/player.gd");
const ENEMY_FIXTURE: &str = include_str!("../fixtures/gdscript/enemies/enemy.gd");
const HEAL_EFFECT_FIXTURE: &str = include_str!("../fixtures/gdscript/effects/heal_effect.gd");

#[tokio::test(flavor = "current_thread")]
async fn test_gdscript_semantic_search_and_analyze_impact() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let workspace_root = temp_dir.path();

    let fixtures = [
        ("player.gd", PLAYER_FIXTURE),
        ("enemies/enemy.gd", ENEMY_FIXTURE),
        ("effects/heal_effect.gd", HEAL_EFFECT_FIXTURE),
    ];

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
    let mut indexer = SimpleIndexer::with_settings(settings.clone());
    indexer
        .enable_semantic_search()
        .expect("enable semantic search");

    for relative in ["player.gd", "enemies/enemy.gd", "effects/heal_effect.gd"] {
        let file_path = workspace_root.join(relative);
        indexer
            .index_file(file_path.to_str().expect("utf8 path"))
            .expect("index fixture file");
    }

    let server = CodeIntelligenceServer::new(indexer);

    let semantic_result = server
        .semantic_search_with_context(Parameters(SemanticSearchWithContextRequest {
            query: "apply damage".to_string(),
            limit: 1,
            threshold: None,
            lang: Some("gdscript".to_string()),
        }))
        .await
        .expect("semantic_search_with_context should succeed");

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
        semantic_text.contains("apply_damage"),
        "expected semantic output to mention apply_damage, got:\n{semantic_text}"
    );

    let apply_damage_symbol_id = semantic_text
        .split("[symbol_id:")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .and_then(|digits| digits.parse::<u32>().ok())
        .expect("semantic output should expose symbol_id for apply_damage");

    let impact_result = server
        .analyze_impact(Parameters(AnalyzeImpactRequest {
            symbol_name: None,
            symbol_id: Some(apply_damage_symbol_id),
            max_depth: 2,
        }))
        .await
        .expect("analyze_impact should succeed");

    let impact_text = impact_result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        impact_text.contains("apply_damage")
            || impact_text.contains("No symbols would be impacted"),
        "expected analyze_impact output to reference apply_damage or report no impacted symbols, got:\n{impact_text}"
    );
}
