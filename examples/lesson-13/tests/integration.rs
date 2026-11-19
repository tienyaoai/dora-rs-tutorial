//! 整合測試：不啟動 dora daemon，直接把假事件餵給 `smoother::run()`。
use dora_node_api::{
    DoraNode, MetadataParameters, Parameter,
    integration_testing::{
        IntegrationTestInput, TestingInput, TestingOptions, TestingOutput, drain_outputs,
        integration_testing_format::{IncomingEvent, InputData, TimedIncomingEvent},
        output_channel,
    },
    serde_json::{Value, json},
};
use smoother::{Config, Stats, run};

/// 造一個 `value` 輸入事件：data 用 JSON 表示，data_type 指定 Arrow 型別。
fn input(t: f64, seq: i64, data: Value, data_type: &str) -> TimedIncomingEvent {
    let mut p = MetadataParameters::new();
    p.insert("seq".into(), Parameter::Integer(seq));
    TimedIncomingEvent {
        time_offset_secs: t,
        event: IncomingEvent::Input {
            id: "value".into(),
            metadata: Some(p),
            data: Some(Box::new(InputData::JsonObject {
                data,
                data_type: Some(json!(data_type)),
            })),
        },
    }
}

fn stop(t: f64) -> TimedIncomingEvent {
    TimedIncomingEvent { time_offset_secs: t, event: IncomingEvent::Stop }
}

/// 跑一次節點，回傳 (統計, 輸出 JSON 陣列)。
fn run_with(events: Vec<TimedIncomingEvent>, cfg: Config) -> (Stats, Vec<Value>) {
    let inputs = TestingInput::Input(IntegrationTestInput::new("smoother".parse().unwrap(), events));
    let (tx, mut rx) = output_channel();
    let options = TestingOptions { skip_output_time_offsets: true }; // 不記時間 → 結果固定
    let (node, events) =
        DoraNode::init_testing(inputs, TestingOutput::ToChannel(tx), options).unwrap();
    let stats = run(node, events, &cfg).unwrap();
    let outputs = drain_outputs(&mut rx).into_iter().map(Value::Object).collect();
    (stats, outputs)
}

#[test]
fn moving_average_and_alarm() {
    let events = vec![
        input(0.00, 1, json!([6.0]), "Float64"),
        input(0.01, 2, json!([12.0]), "Float64"),
        input(0.02, 3, json!([18.0]), "Float64"), // 平均依序 6、9、12，只有 12 > 10 → 1 個 alarm
        stop(0.03),
    ];
    let (stats, out) = run_with(events, Config { window: 3, limit: 10.0 });
    assert_eq!(stats, Stats { received: 3, alarms: 1, bad_inputs: 0 });

    let ids: Vec<&str> = out.iter().map(|o| o["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["avg", "avg", "avg", "alarm"]);
    // avg 依序是 6、9、12
    let avgs: Vec<f64> = out
        .iter()
        .filter(|o| o["id"] == "avg")
        .map(|o| o["data"][0].as_f64().unwrap())
        .collect();
    assert_eq!(avgs, [6.0, 9.0, 12.0]);
    assert_eq!(out[3]["data"][0], "seq=3 avg=12.00 > limit=10.00");
}

#[test]
fn wrong_type_is_skipped_not_panicking() {
    let events = vec![
        input(0.00, 1, json!(["hello"]), "Utf8"), // 字串 → into_vec::<f64> 失敗
        input(0.01, 2, json!([]), "Float64"),     // 空陣列
        input(0.02, 3, json!([1.0, 2.0, 3.0]), "Float64"),
        stop(0.03),
    ];
    let (stats, out) = run_with(events, Config { window: 2, limit: 100.0 });
    assert_eq!(stats, Stats { received: 1, alarms: 0, bad_inputs: 2 });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["data"][0].as_f64(), Some(2.5)); // 視窗 2：只剩 2、3
}

#[test]
fn print_raw_output_format() {
    // 給讀者看輸出 JSON 長什麼樣：cargo test -- --nocapture print_raw
    let (_, out) = run_with(vec![input(0.0, 7, json!([1.5]), "Float64"), stop(0.01)],
        Config { window: 3, limit: 10.0 });
    for o in &out {
        println!("{o}");
    }
}
