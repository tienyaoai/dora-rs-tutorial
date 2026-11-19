//! 第 13 篇：把「邏輯」和「dora 事件迴圈」拆開，兩層都能測。
//!
//! - `Smoother`、`check_alarm`：純 Rust，不碰 dora → 一般 `#[test]` 單元測試。
//! - `run()`：吃一個 `DoraNode` + `EventStream`，正式執行和整合測試共用同一份程式碼。
use std::collections::VecDeque;

use dora_node_api::{
    DoraNode, Event, EventStream, MetadataParameters, Parameter, dora_core::config::DataId,
    get_integer_param, into_vec,
};
use eyre::{Context, bail};

/// 移動平均：保留最近 `window` 個值。
#[derive(Debug)]
pub struct Smoother {
    window: usize,
    buf: VecDeque<f64>,
    sum: f64,
}

impl Smoother {
    pub fn new(window: usize) -> eyre::Result<Self> {
        if window == 0 {
            bail!("window 必須 >= 1");
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window), sum: 0.0 })
    }

    /// 放進一個新值，回傳目前的平均。
    pub fn push(&mut self, x: f64) -> f64 {
        if self.buf.len() == self.window {
            self.sum -= self.buf.pop_front().unwrap();
        }
        self.buf.push_back(x);
        self.sum += x;
        self.sum / self.buf.len() as f64
    }
}

/// 平均值超過門檻就回傳警告字串。
pub fn check_alarm(seq: i64, avg: f64, limit: f64) -> Option<String> {
    (avg > limit).then(|| format!("seq={seq} avg={avg:.2} > limit={limit:.2}"))
}

/// 節點設定（從環境變數讀，YAML 的 `env:` 可以改）。
#[derive(Debug, Clone)]
pub struct Config {
    pub window: usize,
    pub limit: f64,
}

impl Config {
    pub fn from_env() -> eyre::Result<Self> {
        let window = std::env::var("WINDOW").unwrap_or_else(|_| "3".into());
        let limit = std::env::var("LIMIT").unwrap_or_else(|_| "10".into());
        Ok(Self {
            window: window.parse().with_context(|| format!("WINDOW={window:?} 不是整數"))?,
            limit: limit.parse().with_context(|| format!("LIMIT={limit:?} 不是數字"))?,
        })
    }
}

/// 跑完之後的統計，方便測試檢查。
#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub received: usize,
    pub alarms: usize,
    pub bad_inputs: usize,
}

/// 事件迴圈本體。輸入 `value`：Float64 陣列（每個元素都餵進移動平均）。
/// 輸出 `avg`（每筆輸入一個 Float64）與 `alarm`（Utf8 字串，超過門檻才送）。
pub fn run(mut node: DoraNode, mut events: EventStream, cfg: &Config) -> eyre::Result<Stats> {
    let mut smoother = Smoother::new(cfg.window)?;
    let avg_out = DataId::from("avg".to_owned());
    let alarm_out = DataId::from("alarm".to_owned());
    let mut stats = Stats::default();

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "value" => {
                let seq = get_integer_param(&metadata.parameters, "seq").unwrap_or(-1);
                // 型別不對不要 panic：記下來、印到 stderr，繼續處理下一筆
                let values: Vec<f64> = match into_vec::<f64>(&data) {
                    Ok(v) if !v.is_empty() => v,
                    Ok(_) => {
                        eprintln!("[smoother] seq={seq} 收到空陣列，略過");
                        stats.bad_inputs += 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!("[smoother] seq={seq} 型別不對：{e}");
                        stats.bad_inputs += 1;
                        continue;
                    }
                };
                stats.received += 1;
                let mut avg = 0.0;
                for v in values {
                    avg = smoother.push(v);
                }
                let mut p = MetadataParameters::new();
                p.insert("seq".into(), Parameter::Integer(seq));
                node.send_output(avg_out.clone(), p.clone(), vec![avg])?;
                if let Some(msg) = check_alarm(seq, avg, cfg.limit) {
                    stats.alarms += 1;
                    node.send_output(alarm_out.clone(), p, msg)?;
                }
            }
            Event::Input { id, .. } => eprintln!("[smoother] 未知輸入 {id}，略過"),
            Event::Stop(_) => break,
            _ => {}
        }
    }
    eprintln!("[smoother] 結束：{stats:?}");
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_of_partial_window() {
        let mut s = Smoother::new(3).unwrap();
        assert_eq!(s.push(3.0), 3.0);
        assert_eq!(s.push(5.0), 4.0); // 還沒滿 3 個，用實際個數平均
    }

    #[test]
    fn old_values_drop_out() {
        let mut s = Smoother::new(3).unwrap();
        for x in [1.0, 2.0, 3.0] {
            s.push(x);
        }
        assert_eq!(s.push(10.0), 5.0); // (2+3+10)/3
    }

    #[test]
    fn zero_window_is_error() {
        assert!(Smoother::new(0).is_err());
    }

    #[test]
    fn alarm_only_above_limit() {
        assert_eq!(check_alarm(1, 10.0, 10.0), None); // 等於不算超過
        assert_eq!(check_alarm(2, 10.5, 10.0).as_deref(), Some("seq=2 avg=10.50 > limit=10.00"));
    }
}
