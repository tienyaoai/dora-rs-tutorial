//! 示範三種錯誤處理：
//! 1. 可恢復的資料錯誤：記錄後 `continue`，節點繼續活著
//! 2. 致命錯誤：回傳 `Err`，行程以非 0 結束碼退出，交給 YAML 的 restart_policy
//! 3. 重啟後用 `restart_count()` 得知自己是第幾次重生
use dora_node_api::{DoraNode, Event, MetadataParameters};
use eyre::{Context, bail, eyre};

/// 模擬「感測器讀值」處理：5 的倍數當作壞資料（可恢復）
fn process(seq: u64) -> eyre::Result<u64> {
    if seq % 5 == 0 {
        return Err(eyre!("seq={seq} 校驗失敗（模擬壞資料）"));
    }
    Ok(seq * 10)
}

fn main() -> eyre::Result<()> {
    let (mut node, mut events) =
        DoraNode::init_from_env().context("worker 無法連上 dora daemon")?;
    let restart = node.restart_count();

    // CRASH_AT="7,12" 表示：第 0 代在 seq=7 當掉、第 1 代在 seq=12 當掉
    let crash_at: Vec<u64> = std::env::var("CRASH_AT")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let my_crash = crash_at.get(restart as usize).copied();
    println!("worker 啟動：restart_count={restart}，本代預計當掉於 {my_crash:?}");

    let mut ok = 0u32;
    let mut bad = 0u32;
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, data, .. } if id.as_str() == "seq" => {
                // 型別不對屬於「接線錯誤」，直接用 ? 往上拋（致命）
                let seq = u64::try_from(&data).context("seq 不是 u64")?;

                if Some(seq) == my_crash {
                    // 致命錯誤：回傳 Err → main 結束碼非 0 → daemon 依 restart_policy 判斷
                    bail!("worker 第 {restart} 代在 seq={seq} 模擬當機");
                }

                match process(seq) {
                    Ok(v) => {
                        ok += 1;
                        node.send_output("result".into(), MetadataParameters::default(), v)?;
                    }
                    Err(e) => {
                        bad += 1;
                        eprintln!("worker 警告：{e:#}，略過這筆");
                        continue; // 可恢復：不讓一筆壞資料拖垮整個節點
                    }
                }
            }
            Event::InputClosed { id } => println!("worker 輸入 {id} 已關閉"),
            Event::Stop(cause) => {
                println!("worker 收到 Stop({cause:?})");
                break;
            }
            _ => {}
        }
    }
    println!("worker 第 {restart} 代正常結束：成功 {ok} 筆、壞資料 {bad} 筆");
    Ok(())
}
