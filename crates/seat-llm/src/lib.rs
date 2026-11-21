//! 내장 모델(GGUF)을 llama.cpp로 CPU에서 실행한다.
//!
//! - 모델·컨텍스트는 전용 작업 스레드 하나가 가진다(llama.cpp 객체는 스레드 간 이동 불가).
//! - 시스템 프롬프트와 예시는 모든 문장에서 같으므로, 앞부분 KV 캐시를 재사용해
//!   문장마다 새 부분만 계산한다(오래된 PC에서의 속도 대책).
//! - 탐욕(greedy) 디코딩 + 문법 제약 → 같은 문장은 항상 같은 결과.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use seat_interpret::{ChatMessage, LlmEngine};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};

/// 이 CPU에서 모델을 돌릴 수 있는지 (빌드 시 켠 명령어 집합이 있는지).
pub fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::is_x86_feature_detected!("avx")
            && std::is_x86_feature_detected!("avx2")
            && std::is_x86_feature_detected!("fma")
            && std::is_x86_feature_detected!("f16c")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    MissingModel,
    UnsupportedCpu,
    Failed(String),
}

type Reply = Sender<Result<String, String>>;

struct Job {
    messages: Vec<ChatMessage>,
    grammar: String,
    max_tokens: usize,
    reply: Reply,
}

pub struct LlamaEngine {
    tx: std::sync::Mutex<Sender<Job>>,
    /// 모델 파일 크기·이름 (캐시 키에 넣는다)
    pub fingerprint: String,
}

impl LlamaEngine {
    /// 모델을 불러온다(오래 걸림). 다 불러오면 돌아온다.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        if !path.exists() {
            return Err(LoadError::MissingModel);
        }
        if !cpu_supported() {
            return Err(LoadError::UnsupportedCpu);
        }
        let meta = std::fs::metadata(path).map_err(|e| LoadError::Failed(e.to_string()))?;
        let fingerprint = format!("{}:{}", path.file_name().and_then(|n| n.to_str()).unwrap_or(""), meta.len());
        let (tx, rx) = channel::<Job>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let path: PathBuf = path.to_path_buf();
        std::thread::Builder::new()
            .name("llm".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let mut worker = match Worker::new(&path) {
                    Ok(w) => {
                        let _ = ready_tx.send(Ok(()));
                        w
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while let Ok(job) = rx.recv() {
                    let r = worker.run(&job);
                    let _ = job.reply.send(r);
                }
            })
            .map_err(|e| LoadError::Failed(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx: std::sync::Mutex::new(tx), fingerprint }),
            Ok(Err(e)) => Err(LoadError::Failed(e)),
            Err(e) => Err(LoadError::Failed(e.to_string())),
        }
    }
}

impl LlmEngine for LlamaEngine {
    fn complete(&self, messages: &[ChatMessage], grammar: &str, max_tokens: usize) -> Result<String, String> {
        let (reply, rx) = channel();
        self.tx
            .lock()
            .map_err(|e| e.to_string())?
            .send(Job { messages: messages.to_vec(), grammar: grammar.to_string(), max_tokens, reply })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }
}

struct Worker {
    model: &'static LlamaModel,
    backend: &'static LlamaBackend,
    ctx: llama_cpp_2::context::LlamaContext<'static>,
    /// 지금 KV 캐시에 들어 있는 토큰들
    cached: Vec<LlamaToken>,
    n_batch: usize,
}

fn threads() -> i32 {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    n.clamp(1, 8) as i32
}

impl Worker {
    fn new(path: &Path) -> Result<Self, String> {
        let mut backend = LlamaBackend::init().map_err(|e| e.to_string())?;
        backend.void_logs();
        // 모델과 백엔드는 프로그램이 끝날 때까지 쓰므로 수명을 'static으로 둔다.
        let backend: &'static LlamaBackend = Box::leak(Box::new(backend));
        let model = LlamaModel::load_from_file(backend, path, &LlamaModelParams::default()).map_err(|e| e.to_string())?;
        let model: &'static LlamaModel = Box::leak(Box::new(model));
        let n_batch = 512;
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(4096))
            .with_n_batch(n_batch as u32)
            .with_n_threads(threads())
            .with_n_threads_batch(threads());
        let ctx = model.new_context(backend, params).map_err(|e| e.to_string())?;
        Ok(Self { model, backend, ctx, cached: Vec::new(), n_batch })
    }

    fn prompt_text(&self, messages: &[ChatMessage]) -> Result<String, String> {
        let chat: Result<Vec<LlamaChatMessage>, _> =
            messages.iter().map(|m| LlamaChatMessage::new(m.role.clone(), m.content.clone())).collect();
        let chat = chat.map_err(|e| e.to_string())?;
        let tmpl = self.model.chat_template(None).map_err(|e| e.to_string())?;
        self.model.apply_chat_template(&tmpl, &chat, true).map_err(|e| e.to_string())
    }

    fn run(&mut self, job: &Job) -> Result<String, String> {
        let _ = self.backend;
        let text = self.prompt_text(&job.messages)?;
        let tokens = self.model.vocab().tokenize(text.as_bytes(), true, true);
        if tokens.is_empty() || tokens.len() + job.max_tokens >= self.ctx.n_ctx() as usize {
            return Err("프롬프트가 너무 김".into());
        }
        // 앞부분이 같은 만큼 KV 캐시 재사용 (최소 한 토큰은 새로 계산해야 logits가 나온다)
        let mut keep = self.cached.iter().zip(tokens.iter()).take_while(|(a, b)| a == b).count();
        keep = keep.min(tokens.len() - 1);
        if self.ctx.kv_cache_seq_rm(0, Some(keep as u32), None).is_err() {
            self.ctx.clear_kv_cache();
            keep = 0;
        }
        self.cached.truncate(keep);

        let mut batch = LlamaBatch::new(self.n_batch, 1);
        let mut pos = keep;
        for chunk in tokens[keep..].chunks(self.n_batch) {
            batch.clear();
            for (i, &t) in chunk.iter().enumerate() {
                let p = pos + i;
                let last = p == tokens.len() - 1;
                batch.add(t, p as i32, &[0], last).map_err(|e| e.to_string())?;
            }
            self.ctx.decode(&mut batch).map_err(|e| e.to_string())?;
            pos += chunk.len();
            self.cached.extend_from_slice(chunk);
        }

        let grammar = LlamaSampler::grammar(self.model, &job.grammar, "root").map_err(|e| e.to_string())?;
        let mut sampler = LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()]);
        let mut out: Vec<u8> = Vec::new();
        let mut logits_idx = batch.n_tokens() - 1;
        for _ in 0..job.max_tokens {
            let token = sampler.sample(&self.ctx, logits_idx);
            if self.model.vocab().is_eog(token) {
                break;
            }
            out.extend(self.model.vocab().token_to_piece(token, false, None));
            batch.clear();
            batch.add(token, pos as i32, &[0], true).map_err(|e| e.to_string())?;
            self.ctx.decode(&mut batch).map_err(|e| e.to_string())?;
            self.cached.push(token);
            pos += 1;
            logits_idx = 0;
        }
        String::from_utf8(out).map_err(|e| e.to_string())
    }
}
