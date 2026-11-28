// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! llama.cpp load and context settings read from the environment (UKH1 U2,
//! plan D2 to D5 and D7).
//!
//! Every setting defaults to the behaviour before 8.67.0 when its variable is
//! unset or empty: no tensor override, llama.cpp's own mmap default (the
//! setter is not called at all), llama.cpp's thread defaults, no rope scaling
//! change and no top_k sampler. Parsing is pure (`from_lookup`) so it is
//! tested without a GPU; `from_env` is the one reader the engine
//! constructions in `main.rs` and `api/websocket/inference.rs` share.

use anyhow::{anyhow, Result};
use llama_cpp_2::context::params::{LlamaContextParams, RopeScalingType};
use std::ffi::CString;

/// The settings, parsed. `Default` is today's behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LlamaTuning {
    /// `LLAMA_CPU_MOE_LAYERS`: routed-expert tensors of layers `0..n` go to
    /// the CPU buffer type. 0 = no override.
    pub cpu_moe_layers: u32,
    /// `LLAMA_USE_MMAP=false` only. `true` and unset both leave llama.cpp's
    /// default (AUTO) alone.
    pub disable_mmap: bool,
    /// `LLAMA_THREADS`.
    pub n_threads: Option<i32>,
    /// `LLAMA_THREADS_BATCH`.
    pub n_threads_batch: Option<i32>,
    /// `ROPE_SCALING=yarn`.
    pub rope_yarn: bool,
    /// `YARN_ORIG_CTX`.
    pub yarn_orig_ctx: Option<u32>,
    /// `LLAMA_TOP_K` (> 0). Requests' own top_k (hard-coded 40 by every
    /// builder) is never used.
    pub top_k: Option<i32>,
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn parse_positive<T: std::str::FromStr + PartialOrd + Default>(
    name: &str,
    v: Option<String>,
) -> Result<Option<T>> {
    match non_empty(v) {
        None => Ok(None),
        Some(s) => {
            let n: T = s
                .parse()
                .map_err(|_| anyhow!("{name} must be a whole number, got {s:?}"))?;
            if n <= T::default() {
                return Err(anyhow!("{name} must be greater than 0, got {s:?}"));
            }
            Ok(Some(n))
        }
    }
}

impl LlamaTuning {
    /// Parse from any lookup (tests pass a map; production passes the
    /// process environment). A value that cannot be parsed refuses the start
    /// rather than falling back silently.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let cpu_moe_layers = match non_empty(get("LLAMA_CPU_MOE_LAYERS")) {
            None => 0,
            Some(s) => s.parse::<u32>().map_err(|_| {
                anyhow!("LLAMA_CPU_MOE_LAYERS must be a whole number of layers, got {s:?}")
            })?,
        };
        let disable_mmap = match non_empty(get("LLAMA_USE_MMAP")) {
            None => false,
            Some(s) if s.eq_ignore_ascii_case("false") => true,
            Some(s) if s.eq_ignore_ascii_case("true") => false,
            Some(s) => return Err(anyhow!("LLAMA_USE_MMAP must be true or false, got {s:?}")),
        };
        let rope_yarn = match non_empty(get("ROPE_SCALING")) {
            None => false,
            Some(s) if s.eq_ignore_ascii_case("yarn") => true,
            Some(s) if s.eq_ignore_ascii_case("none") => false,
            Some(s) => {
                return Err(anyhow!(
                    "ROPE_SCALING must be yarn or none (unset = off), got {s:?}"
                ))
            }
        };
        Ok(Self {
            cpu_moe_layers,
            disable_mmap,
            n_threads: parse_positive("LLAMA_THREADS", get("LLAMA_THREADS"))?,
            n_threads_batch: parse_positive("LLAMA_THREADS_BATCH", get("LLAMA_THREADS_BATCH"))?,
            rope_yarn,
            yarn_orig_ctx: parse_positive("YARN_ORIG_CTX", get("YARN_ORIG_CTX"))?,
            top_k: parse_positive("LLAMA_TOP_K", get("LLAMA_TOP_K"))?,
        })
    }

    /// The process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// The value to pass to `LlamaModelParams::with_use_mmap`, or `None` when
    /// the setter must not be called (keeps llama.cpp's AUTO load mode).
    pub fn use_mmap_override(&self) -> Option<bool> {
        if self.disable_mmap {
            Some(false)
        } else {
            None
        }
    }

    /// The CPU buffer-type override pattern for `cpu_moe_layers`, as a C
    /// string the caller keeps alive until `load_from_file` returns.
    pub fn cpu_moe_override(&self) -> Option<CString> {
        cpu_moe_pattern(self.cpu_moe_layers)
            .map(|p| CString::new(p).expect("pattern has no NUL byte"))
    }

    /// Apply the context settings. Unset fields leave the params untouched.
    pub fn apply_to_context(&self, mut params: LlamaContextParams) -> LlamaContextParams {
        if let Some(n) = self.n_threads {
            params = params.with_n_threads(n);
        }
        if let Some(n) = self.n_threads_batch {
            params = params.with_n_threads_batch(n);
        }
        if self.rope_yarn {
            params = params.with_rope_scaling_type(RopeScalingType::Yarn);
        }
        if let Some(n) = self.yarn_orig_ctx {
            params = params.with_yarn_orig_ctx(n);
        }
        params
    }

    /// One line for the start log.
    pub fn describe(&self) -> String {
        format!(
            "cpu_moe_layers={} pattern={} use_mmap={} threads={} threads_batch={} rope_scaling={} yarn_orig_ctx={} top_k={}",
            self.cpu_moe_layers,
            cpu_moe_pattern(self.cpu_moe_layers).unwrap_or_else(|| "none".into()),
            if self.disable_mmap { "false" } else { "default" },
            self.n_threads.map_or("default".into(), |n| n.to_string()),
            self.n_threads_batch.map_or("default".into(), |n| n.to_string()),
            if self.rope_yarn { "yarn" } else { "default" },
            self.yarn_orig_ctx.map_or("default".into(), |n| n.to_string()),
            self.top_k.map_or("none".into(), |n| n.to_string()),
        )
    }
}

/// `blk\.(0|1|...|n-1)\.ffn_(up|down|gate)_(ch|)exps`, the routed-expert
/// tensors of the first `n` layers (llama.cpp's `--n-cpu-moe`). The `\.`
/// after the layer number keeps `blk.1` from matching `blk.10` to `blk.19`
/// under llama.cpp's unanchored `std::regex_search`. `None` for 0.
pub fn cpu_moe_pattern(n: u32) -> Option<String> {
    if n == 0 {
        return None;
    }
    let layers: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    Some(format!(
        r"blk\.({})\.ffn_(up|down|gate)_(ch|)exps",
        layers.join("|")
    ))
}

impl LlamaTuning {
    /// The top_k sampler `LLAMA_TOP_K` asks for, or `None` (today: no top_k
    /// sampler). The chain places it before top_p (llama.cpp's order).
    pub fn top_k_sampler(&self) -> Option<llama_cpp_2::sampling::LlamaSampler> {
        self.top_k.map(llama_cpp_2::sampling::LlamaSampler::top_k)
    }

    /// Model params for the load: `base` (with `with_n_gpu_layers` etc.
    /// already applied), then the mmap setting, then pinned, then the CPU
    /// expert override. `cpu_override` is the string from
    /// [`cpu_moe_override`](Self::cpu_moe_override): the params keep a raw
    /// pointer into it, so the CALLER must keep it alive until
    /// `LlamaModel::load_from_file` has returned and the params are dropped.
    pub fn model_params(
        &self,
        base: llama_cpp_2::model::params::LlamaModelParams,
        cpu_override: Option<&std::ffi::CStr>,
    ) -> std::pin::Pin<Box<llama_cpp_2::model::params::LlamaModelParams>> {
        let base = match self.use_mmap_override() {
            Some(v) => base.with_use_mmap(v),
            None => base,
        };
        let mut pinned = Box::pin(base);
        if let Some(p) = cpu_override {
            pinned.as_mut().add_cpu_buft_override(p);
        }
        pinned
    }
}
