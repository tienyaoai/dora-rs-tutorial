//! ONNX Runtime（ort crate）推論節點：介面和 infer-candle 一樣（image → result），
//! 模型是 ONNX Model Zoo 的 mnist-12.onnx（輸入固定 (1,1,28,28)，所以一次只推一張）。
//!   MODEL        .onnx 路徑
//!   ORT_THREADS  ONNX Runtime 的 intra-op 執行緒數（預設 1）
use dora_node_api::{DoraNode, Event, MetadataParameters, Parameter, dora_core::config::DataId,
                    get_integer_param};
use lesson_17::{Lat, PIXELS, env_or, top1};
use ort::{session::Session, value::TensorRef};
use std::time::Instant;

fn main() -> eyre::Result<()> {
    let model: String = env_or("MODEL", "models/mnist-12.onnx".to_string());
    let threads: usize = env_or("ORT_THREADS", 1usize);
    let t = Instant::now();
    // ort 的錯誤型別 Error<SessionBuilder> 不是 Send/Sync，不能直接用 ? 轉 eyre，所以先轉成字串
    let env = ort::init().with_name("dora-infer-ort").build().map_err(|e| eyre::eyre!("ort: {e}"))?;
    let mut sess = Session::builder(&env).map_err(|e| eyre::eyre!("ort: {e}"))?
        .with_intra_threads(threads).map_err(|e| eyre::eyre!("ort: {e}"))?
        .commit_from_file(&model).map_err(|e| eyre::eyre!("ort: {e}"))?;
    let input_name = sess.inputs()[0].name().to_string();
    println!("[ort] 模型 {model} 載入 {:.2} ms；輸入 {input_name}；ORT_THREADS={threads}", t.elapsed().as_secs_f64() * 1e3);

    let (mut node, mut events) = DoraNode::init_from_env()?;
    let out = DataId::from("result".to_owned());
    let mut x = vec![0f32; PIXELS]; // 重複使用同一塊輸入 buffer，不每張配置
    let (mut n, mut n_queue_drop) = (0u64, 0u64);
    let mut lat = Lat::default();
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } if id.as_str() == "image" => {
                let px: &[u8] = (&data).try_into()?;
                eyre::ensure!(px.len() == PIXELS, "影像大小 {} ≠ {PIXELS}", px.len());
                // mnist-12 直接吃 0..255 的 float（bench 實測 /255 與否正確率幾乎一樣）
                for (d, s) in x.iter_mut().zip(px) { *d = *s as f32; }
                let t = Instant::now();
                let outputs = sess.run(ort::inputs![TensorRef::from_array_view(([1usize, 1, 28, 28], &*x))?])?;
                let (_shape, logits) = outputs[0].try_extract_tensor::<f32>()?;
                let (pred, score) = top1(logits);
                let infer_ns = t.elapsed().as_nanos() as u64;
                lat.push(infer_ns);
                let pm = &metadata.parameters;
                let mut p = MetadataParameters::new();
                for k in ["seq", "label", "t_ns"] {
                    p.insert(k.into(), Parameter::Integer(get_integer_param(pm, k).unwrap_or(-1)));
                }
                p.insert("pred".into(), Parameter::Integer(pred as i64));
                p.insert("score".into(), Parameter::Float(score as f64));
                p.insert("infer_us".into(), Parameter::Integer((infer_ns / 1000) as i64));
                p.insert("batch".into(), Parameter::Integer(1));
                p.insert("backend".into(), Parameter::String("ort".into()));
                let logits = logits.to_vec();
                drop(outputs);
                node.send_output(out.clone(), p, logits)?; // 這裡送 logits（未做 softmax）
                n += 1;
                for (_, d) in events.drain_drop_counts() { n_queue_drop += d; }
            }
            Event::Stop(_) => break,
            _ => {}
        }
    }
    println!("[ort] 結束：推論 {n} 張，每次呼叫 {}；dora 佇列丟棄 {n_queue_drop}", lat.summary());
    Ok(())
}
