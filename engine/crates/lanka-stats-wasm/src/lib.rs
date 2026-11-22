//! `lanka-stats` for the browser (11 §8, UI contract §7 `StatsKernel`).
//!
//! A plain C-ABI WebAssembly module — no wasm-bindgen, no JS glue to
//! generate: the host allocates `f64` buffers with `lsw_alloc`, calls a
//! kernel, reads the output buffer and frees. `js/lanka-stats.mjs` wraps it
//! in the `StatsKernel` interface. Every kernel here is a thin layer over
//! [`kernels`], which the native engine and the parity test call directly,
//! so a WebAssembly result can be compared with the native one bit for bit
//! (`tests/wasm_parity.rs`).
//!
//! The `extern "C"` functions are the crate's only `unsafe` (raw pointers
//! from the host); each checks for null and trusts the lengths it is given,
//! as any WebAssembly export must. Second audited exception to the
//! workspace's `unsafe_code = "deny"` (the first is `lanka-grid/src/mmap.rs`).

pub mod kernels;

#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod exports {
    use crate::kernels;
    use std::alloc::{Layout, alloc, dealloc};

    fn layout(bytes: usize) -> Option<Layout> {
        Layout::from_size_align(bytes.max(8), 8).ok()
    }

    /// Allocates `bytes` (8-byte aligned); 0 on failure.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_alloc(bytes: usize) -> *mut u8 {
        match layout(bytes) {
            // SAFETY: the layout has a non-zero size.
            Some(l) => unsafe { alloc(l) },
            None => core::ptr::null_mut(),
        }
    }

    /// Frees a buffer from `lsw_alloc` with the same `bytes`.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_free(ptr: *mut u8, bytes: usize) {
        if let (false, Some(l)) = (ptr.is_null(), layout(bytes)) {
            // SAFETY: `ptr` came from `lsw_alloc(bytes)`.
            unsafe { dealloc(ptr, l) }
        }
    }

    fn input<'a>(ptr: *const f64, n: usize) -> &'a [f64] {
        if ptr.is_null() || n == 0 {
            return &[];
        }
        // SAFETY: the host passes a live buffer of `n` f64 it allocated here.
        unsafe { core::slice::from_raw_parts(ptr, n) }
    }

    fn output(out: *mut f64, cap: usize, v: &[f64]) -> u32 {
        if out.is_null() {
            return 0;
        }
        let n = v.len().min(cap);
        // SAFETY: the host passes a live, writable buffer of `cap` f64.
        let dst = unsafe { core::slice::from_raw_parts_mut(out, n) };
        dst.copy_from_slice(&v[..n]);
        u32::try_from(v.len()).unwrap_or(u32::MAX)
    }

    /// Inner breaks of `values` (`method`: 0 quantile, 1 equal, 2 jenks,
    /// 3 stddev, 4 headtail, 5 pretty). Returns the number of breaks.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_classify(
        values: *const f64,
        n: usize,
        method: u32,
        k: u32,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(out, cap, &kernels::classify(input(values, n), method, k))
    }

    /// `[n, r, ci_lo, ci_hi, p, rho, rho_ci_lo, rho_ci_hi, rho_p]`.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_correlate(
        x: *const f64,
        y: *const f64,
        n: usize,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(out, cap, &kernels::correlate(input(x, n), input(y, n)))
    }

    /// OLS of `y` on `k` predictor columns stored one after the other in
    /// `xs` (`k·n` values) plus an intercept: `[status, n, p, coef…, se_hc3…,
    /// p_hc3…, r2, adj_r2, sigma]` (status 0 = ok, 1 = too few rows,
    /// 2 = singular design).
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_regress(
        y: *const f64,
        xs: *const f64,
        n: usize,
        k: usize,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(
            out,
            cap,
            &kernels::regress(input(y, n), input(xs, n * k), k),
        )
    }

    /// `[n, r, rho, intercept, slope]` on transformed complete pairs
    /// (transform codes: 0 identity, 1 log, 2 log1p, 3 sqrt, 4 zscore, 5 rank).
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_preview_relate(
        x: *const f64,
        y: *const f64,
        n: usize,
        tx: u32,
        ty: u32,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(
            out,
            cap,
            &kernels::preview(input(x, n), input(y, n), tx, ty),
        )
    }

    /// `[n, missing, mean, sd, min, max, p5, p25, p50, p75, p95, mad,
    /// skewness, kurtosis, non_positive]`.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_describe(values: *const f64, n: usize, out: *mut f64, cap: usize) -> u32 {
        output(out, cap, &kernels::describe(input(values, n)))
    }

    /// Hexbin: `[cells, radius, cx…, cy…, count…]`.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_hexbin(
        x: *const f64,
        y: *const f64,
        n: usize,
        radius: f64,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(out, cap, &kernels::hexbin(input(x, n), input(y, n), radius))
    }

    /// Bivariate classes `class_y·k + class_x` (255 = missing) as f64, after
    /// `k − 1` x breaks and `k − 1` y breaks: `[nx, ny, xb…, yb…, class…]`.
    #[unsafe(no_mangle)]
    pub extern "C" fn lsw_bivariate(
        x: *const f64,
        y: *const f64,
        n: usize,
        method: u32,
        k: u32,
        out: *mut f64,
        cap: usize,
    ) -> u32 {
        output(
            out,
            cap,
            &kernels::bivariate(input(x, n), input(y, n), method, k),
        )
    }
}
