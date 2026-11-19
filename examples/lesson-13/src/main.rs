//! smoother 節點的執行檔：只負責初始化，邏輯都在 lib.rs。
use dora_node_api::DoraNode;

fn main() -> eyre::Result<()> {
    let cfg = smoother::Config::from_env()?;
    // init_from_env：被 dora daemon 啟動時連 daemon；
    // 設了 DORA_TEST_WITH_INPUTS 時改成「整合測試模式」，從 JSON 檔讀輸入。
    let (node, events) = DoraNode::init_from_env()?;
    smoother::run(node, events, &cfg)?;
    Ok(())
}
