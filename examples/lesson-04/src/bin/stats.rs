use dora_node_api::{
    DoraNode, Event, MetadataParameters, StopCause, dora_core::config::DataId,
};
use std::time::Duration;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let summary = DataId::from("summary".to_owned());

    // 節點自己的狀態：就放在迴圈外的區域變數
    let (mut count, mut sum, mut max) = (0u32, 0.0f64, f64::MIN);

    // recv_timeout：最多等 500 ms；逾時不會回 None，而是回一個 Event::Error
    while let Some(event) = events.recv_timeout(Duration::from_millis(500)) {
        match event {
            Event::Input { id, data, .. } => match id.as_str() {
                "reading" => {
                    let v = f64::try_from(&data)?; // Arrow 陣列 → f64
                    count += 1;
                    sum += v;
                    max = max.max(v);
                    println!("stats 收到 reading={v}（累計 {count} 筆）");
                }
                "report" => {
                    let text = if count == 0 {
                        "尚無資料".to_string()
                    } else {
                        format!("{count} 筆，平均 {:.2}，最大 {max:.1}", sum / count as f64)
                    };
                    node.send_output(summary.clone(), MetadataParameters::default(), text.as_str())?;
                }
                other => println!("stats 收到沒預期的輸入 {other}"),
            },
            Event::InputClosed { id } => {
                println!("stats：輸入 {id} 已關閉");
                if id.as_str() == "reading" {
                    // 感測器沒資料了：送最後一份摘要，然後自己收工
                    let text = format!("最終：{count} 筆，總和 {sum:.1}");
                    node.send_output(summary.clone(), MetadataParameters::default(), text.as_str())?;
                    break;
                }
            }
            Event::Stop(cause) => {
                match cause {
                    StopCause::Manual => println!("stats：被手動停止（dora stop / Ctrl-C）"),
                    StopCause::AllInputsClosed => println!("stats：所有輸入都關閉了"),
                    _ => println!("stats：停止（其他原因）"),
                }
                break;
            }
            // 逾時（recv_timeout 沒等到事件）會以 Error 出現
            Event::Error(msg) => println!("stats：Error 事件：{msg}"),
            // 其他事件（NodeFailed、ParamUpdate…）先印出來看看
            other => println!("stats：其他事件 {other:?}"),
        }
    }
    println!("stats 結束（共 {count} 筆）");
    Ok(())
}
