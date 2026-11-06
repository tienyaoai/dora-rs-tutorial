use dora_node_api::{
    DoraNode, Event, Parameter, arrow_v59::array::{Array, Float64Array, StringArray, StructArray, UInt32Array},
    get_bool_param, get_integer_param, get_string_param,
};

fn main() -> eyre::Result<()> {
    let (_node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => {
                // 不用解開資料，就能看到它的 Arrow 型別與長度
                println!(
                    "── 輸入 {id}：型別 {}，長度 {}，null 數 {}",
                    data.type_name(),
                    data.len(),
                    data.null_count()
                );
                let p = &metadata.parameters;
                match id.as_str() {
                    "temperature" => {
                        let t = f64::try_from(&data)?;
                        println!("  值 = {t}");
                        // 型別安全的 getter：型別不符或鍵不存在都回 None
                        println!("  unit  = {:?}", get_string_param(p, "unit"));
                        println!("  seq   = {:?}", get_integer_param(p, "seq"));
                        println!("  valid = {:?}", get_bool_param(p, "valid"));
                        println!("  seq 當字串讀 = {:?}（型別不符 → None）", get_string_param(p, "seq"));
                        println!("  不存在的鍵   = {:?}", get_integer_param(p, "nope"));
                        // 清單類參數沒有 getter，直接 match 變體
                        if let Some(Parameter::ListFloat(c)) = p.get("calibration") {
                            println!("  calibration = {c:?}");
                        }
                        if let Some(Parameter::ListString(t)) = p.get("tags") {
                            println!("  tags = {t:?}");
                        }
                        // 型別錯誤示範：把 Float64 當 i32 讀
                        match i32::try_from(&data) {
                            Ok(v) => println!("  （意外）當 i32 讀到 {v}"),
                            Err(e) => println!("  把 Float64 當 i32 讀 → 錯誤（不是 panic）：{e}"),
                        }
                        // 發送時間戳（HLC）也在 metadata 裡
                        println!("  metadata 版本 = {}", metadata.metadata_version());
                    }
                    "waveform" => {
                        let v: Vec<f32> = Vec::try_from(&data)?; // 複製成 Vec
                        let s: &[f32] = TryFrom::try_from(&data)?; // 或零拷貝借用 slice
                        println!("  Vec 長度 {}，slice 長度 {}，最大值 {:.3}", v.len(), s.len(),
                            s.iter().cloned().fold(f32::MIN, f32::max));
                        // 多元素陣列不能當純量讀
                        println!("  當純量 f32 讀 → {:?}", f32::try_from(&data).is_err());
                    }
                    "table" => {
                        // as_array() 借出底層 Arrow 陣列；StructArray::from 由 ArrayData 轉換
                        let table = StructArray::from(data.as_array().to_data());
                        println!("  欄位：{:?}", table.column_names());
                        let ids = table.column_by_name("id").unwrap()
                            .as_any().downcast_ref::<UInt32Array>().unwrap();
                        let temps = table.column_by_name("temp").unwrap()
                            .as_any().downcast_ref::<Float64Array>().unwrap();
                        let labels = table.column_by_name("label").unwrap()
                            .as_any().downcast_ref::<StringArray>().unwrap();
                        for i in 0..table.len() {
                            println!("  列 {i}: id={} temp={} label={}", ids.value(i), temps.value(i), labels.value(i));
                        }
                    }
                    "ping" => {
                        println!("  note = {:?}", get_string_param(p, "note"));
                    }
                    _ => {}
                }
            }
            Event::InputClosed { id } => println!("（輸入 {id} 已關閉）"),
            Event::Stop(c) => { println!("consumer 停止：{c:?}"); break; }
            _ => {}
        }
    }
    println!("consumer 結束");
    Ok(())
}
