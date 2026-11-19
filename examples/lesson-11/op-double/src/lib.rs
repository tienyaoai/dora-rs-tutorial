//! Operator：每收到一個 tick，計數器 +1，送出「計數 × 2」。
use dora_operator_api::{
    DoraOperator, DoraOutputSender, DoraStatus, Event, IntoArrow, register_operator,
};

register_operator!(Double);

/// 狀態放在 struct 裡；runtime 用 Default 建立它（不能有參數）。
#[derive(Default)]
struct Double {
    count: i64,
    said_hello: bool,
}

impl DoraOperator for Double {
    fn on_event(
        &mut self,
        event: &Event,
        out: &mut DoraOutputSender,
    ) -> Result<DoraStatus, String> {
        match event {
            Event::Input { id, .. } if *id == "tick" => {
                if !self.said_hello {
                    self.said_hello = true;
                    println!(
                        "[op double] pid={} thread={:?}",
                        std::process::id(),
                        std::thread::current().id()
                    );
                }
                self.count += 1;
                out.send("value", (self.count * 2).into_arrow())?;
                Ok(DoraStatus::Continue)
            }
            Event::InputParseError { id, error } => Err(format!("{id}: {error}")),
            Event::Stop => Ok(DoraStatus::Stop),
            _ => Ok(DoraStatus::Continue),
        }
    }
}
