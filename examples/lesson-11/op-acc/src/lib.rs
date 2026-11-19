//! Operator：把收到的 i64 累加，送出累計和。
use dora_operator_api::{
    DoraOperator, DoraOutputSender, DoraStatus, Event, IntoArrow, register_operator,
};

register_operator!(Acc);

#[derive(Default)]
struct Acc {
    total: i64,
    said_hello: bool,
}

impl DoraOperator for Acc {
    fn on_event(
        &mut self,
        event: &Event,
        out: &mut DoraOutputSender,
    ) -> Result<DoraStatus, String> {
        match event {
            Event::Input { id, data, .. } if *id == "value" => {
                if !self.said_hello {
                    self.said_hello = true;
                    println!(
                        "[op acc] pid={} thread={:?}",
                        std::process::id(),
                        std::thread::current().id()
                    );
                }
                // DoraArray → i64：用 TryFrom，型別不符就回傳錯誤字串
                let v: i64 = data
                    .try_into()
                    .map_err(|e| format!("value 不是 i64: {e}"))?;
                self.total += v;
                out.send("total", self.total.into_arrow())?;
                Ok(DoraStatus::Continue)
            }
            Event::InputClosed { id } if *id == "value" => Ok(DoraStatus::Stop),
            Event::Stop => Ok(DoraStatus::Stop),
            _ => Ok(DoraStatus::Continue),
        }
    }
}
