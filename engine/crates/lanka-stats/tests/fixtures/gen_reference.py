#!/usr/bin/env python3
"""Reference fixtures for lanka-stats (WP-29, 11 §13).

R is not installed on the build machine, so the reference is Python:
numpy 2.5.5, scipy 1.18.1, statsmodels 0.15.0 (install into a venv:
`python3 -m venv v && v/bin/pip install numpy==2.5.5 scipy==1.18.1 statsmodels==0.15.0`).
Run from this directory:  v/bin/python -I gen_reference.py > reference.json

What each block checks (tolerance in tests/reference.rs: |a−b| ≤ 1e-8·|b| + 1e-12):
- special: scipy.special / scipy.stats distribution functions
- describe: numpy percentiles (type 7), scipy skew/kurtosis(bias=False), MAD
- correlate: scipy.stats.pearsonr (+ Fisher-z CI), spearmanr
- regress: statsmodels OLS (HC3), OLSInfluence (leverage, Cook's D),
  variance_inflation_factor, het_breuschpagan (Koenker), RLM(HuberT) on the
  textbook Stack loss and Longley data and Anscombe's quartet
- compare: scipy ttest_ind(equal_var=False), mannwhitneyu(asymptotic), the
  Hodges–Lehmann shift (median of np.subtract.outer), Cohen's d
- classify: numpy quantiles, equal intervals, Fisher–Jenks by an independent
  dynamic programme, head/tail and stddev breaks
- effn: Dutilleul's M̂ with explicit n×n matrices (independent of the
  closed-form pass in effn.rs)
- bootstrap: the SplitMix64 counter stream of rng.rs ported below, so seeded
  resampling must agree exactly (bit for bit)
"""

import json
import math
import struct

import numpy as np
import scipy
import scipy.special as sp
import scipy.stats as ss
import statsmodels
import statsmodels.api as sm
from statsmodels.stats.diagnostic import het_breuschpagan
from statsmodels.stats.outliers_influence import OLSInfluence, variance_inflation_factor

M64 = (1 << 64) - 1
GOLDEN = 0x9E3779B97F4A7C15


def mix(z):
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


class Stream:
    def __init__(self, seed, stream):
        self.key = mix(seed ^ mix((stream + GOLDEN) & M64))
        self.i = 0

    def next_u64(self):
        self.i += 1
        return mix((self.key + self.i * GOLDEN) & M64)

    def below(self, n):
        return (self.next_u64() * n) >> 64


def bits(vals):
    """IEEE-754 bit patterns as hex: exact values survive any JSON parser."""
    return ["%016x" % struct.unpack("<Q", struct.pack("<d", v))[0] for v in vals]


def q7(sorted_vals, p):
    """Type-7 quantile with exactly the arithmetic of describe::quantile_sorted."""
    s = sorted_vals
    n = len(s)
    if n == 1:
        return float(s[0])
    h = (n - 1) * min(max(p, 0.0), 1.0)
    lo = math.floor(h)
    hi = min(lo + 1, n - 1)
    return float(s[lo] + (h - lo) * (s[hi] - s[lo]))


def seq_mean(v):
    """Left-to-right sum, as Rust's iter().sum() (numpy sums pairwise)."""
    t = 0.0
    for x in v:
        t += x
    return t / len(v)


def bootstrap(a, b, stat, reps, level, seed, stream):
    s = Stream(seed, stream)
    f = (lambda v: q7(sorted(v), 0.5)) if stat == "median" else seq_mean
    out = []
    for _ in range(reps):
        ra = [a[s.below(len(a))] for _ in range(len(a))]
        x = f(ra)
        if b is not None:
            rb = [b[s.below(len(b))] for _ in range(len(b))]
            x -= f(rb)
        out.append(float(x))
    out.sort()
    alpha = (1 - level) / 2
    return [q7(out, alpha), q7(out, 1 - alpha)]


def jenks(values, k):
    s = sorted(values)
    n = len(s)
    best = {}

    def ssd(a, b):
        seg = np.array(s[a:b])
        return float(((seg - seg.mean()) ** 2).sum())

    cost = [[math.inf] * (k + 1) for _ in range(n + 1)]
    cut = [[0] * (k + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        cost[i][1] = ssd(0, i)
    for c in range(2, k + 1):
        for i in range(c, n + 1):
            for j in range(c - 1, i):
                v = cost[j][c - 1] + ssd(j, i)
                if v < cost[i][c]:
                    cost[i][c] = v
                    cut[i][c] = j
    br = []
    i = n
    for c in range(k, 1, -1):
        i = cut[i][c]
        br.append(s[i])
    del best
    return sorted(br)


def headtail(values, k):
    head = sorted(values)
    out = []
    while len(out) < k - 1 and len(head) > 1:
        m = float(np.mean(head))
        nxt = [v for v in head if v > m]
        if not nxt or len(nxt) == len(head):
            break
        out.append(m)
        share = len(nxt) / len(head)
        head = nxt
        if share > 0.4:
            break
    return out


def stddev_breaks(values, k):
    v = np.asarray(values)
    m, sd = v.mean(), v.std(ddof=1)
    half = (k - 1) / 2
    br = [m + (i - half + 0.5) * sd for i in range(k - 1)]
    return [float(b) for b in br if v.min() < b < v.max()]


def haversine(lon1, lat1, lon2, lat2):
    r = math.pi / 180
    p1, p2 = lat1 * r, lat2 * r
    a = math.sin((p2 - p1) / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin((lon2 - lon1) * r / 2) ** 2
    return 2 * 6371.0088 * math.asin(math.sqrt(min(a, 1.0)))


def effn(x, y, lon, lat, classes=13):
    n = len(x)
    xs, ys = np.asarray(x) - np.mean(x), np.asarray(y) - np.mean(y)
    D = np.array([[haversine(lon[i], lat[i], lon[j], lat[j]) for j in range(n)] for i in range(n)])
    w = D.max() / classes
    C = np.minimum(np.floor(D / w), classes - 1).astype(int)
    off = ~np.eye(n, dtype=bool)

    def sigma(v):
        S = np.zeros((n, n))
        for c in range(classes):
            m = (C == c) & off
            cnt = m.sum()
            cov = (np.outer(v, v)[m].sum() / cnt) if cnt else 0.0
            S[m] = cov
        np.fill_diagonal(S, (v * v).mean())
        return S

    P = np.eye(n) - np.ones((n, n)) / n
    Bx, By = P @ sigma(xs) @ P, P @ sigma(ys) @ P
    M = 1 + np.trace(Bx) * np.trace(By) / np.trace(Bx @ By)
    return float(min(max(M, 2.0), n))


def ols_block(name, y, X, names):
    Xc = sm.add_constant(np.column_stack(X), has_constant="add")
    fit = sm.OLS(y, Xc).fit()
    hc3 = sm.OLS(y, Xc).fit(cov_type="HC3")
    infl = OLSInfluence(fit)
    df = fit.df_resid
    q = ss.t.ppf(0.975, df)
    se3 = hc3.bse
    t3 = fit.params / se3
    rlm = sm.RLM(y, Xc, M=sm.robust.norms.HuberT()).fit(conv="coefs", tol=1e-15, maxiter=2000)
    bp = het_breuschpagan(fit.resid, Xc, robust=True)
    vif = [variance_inflation_factor(Xc, i) for i in range(1, Xc.shape[1])] if Xc.shape[1] > 2 else []
    return {
        "name": name,
        "terms": names,
        "y": list(map(float, y)),
        "x": [list(map(float, c)) for c in X],
        "coef": list(map(float, fit.params)),
        "se": list(map(float, fit.bse)),
        "se_hc3": list(map(float, se3)),
        "p_hc3": [float(2 * ss.t.sf(abs(t), df)) for t in t3],
        "ci_hc3": [[float(b - q * s), float(b + q * s)] for b, s in zip(fit.params, se3)],
        "sigma": float(np.sqrt(fit.scale)),
        "r2": float(fit.rsquared),
        "adj_r2": float(fit.rsquared_adj),
        "leverage": list(map(float, infl.hat_matrix_diag)),
        "cooks": list(map(float, infl.cooks_distance[0])),
        "vif": list(map(float, vif)),
        "bp_lm": float(bp[0]),
        "bp_p": float(bp[1]),
        "huber_coef": list(map(float, rlm.params)),
        "huber_scale": float(rlm.scale),
    }


def corr_block(name, x, y):
    pr = ss.pearsonr(x, y)
    ci = pr.confidence_interval(0.95)
    sr = ss.spearmanr(x, y)
    n = len(x)
    z = math.atanh(sr.statistic)
    se = math.sqrt(1.06 / (n - 3))
    zq = ss.norm.ppf(0.975)
    return {
        "name": name,
        "x": list(map(float, x)),
        "y": list(map(float, y)),
        "pearson_r": float(pr.statistic),
        "pearson_p": float(pr.pvalue),
        "pearson_ci": [float(ci.low), float(ci.high)],
        "spearman_rho": float(sr.statistic),
        "spearman_p": float(sr.pvalue),
        "spearman_ci": [math.tanh(z - zq * se), math.tanh(z + zq * se)],
    }


def main():
    rng = np.random.default_rng(20261122)
    out = {
        "generator": "gen_reference.py",
        "reference": f"python numpy {np.__version__}, scipy {scipy.__version__}, statsmodels {statsmodels.__version__}",
    }

    # ── special functions ────────────────────────────────────────────────
    spc = []
    for df in [1, 2, 3, 5, 10, 30, 120, 1000]:
        for p in [0.6, 0.9, 0.95, 0.975, 0.995, 0.9995]:
            spc.append({"f": "t_ppf", "a": [p, df], "v": float(ss.t.ppf(p, df))})
        for t in [0.1, 1.0, 2.5, 7.0, 25.0]:
            spc.append({"f": "t_p2", "a": [t, df], "v": float(2 * ss.t.sf(t, df))})
    for p in [1e-10, 0.001, 0.025, 0.3, 0.5, 0.8, 0.975, 0.999999]:
        spc.append({"f": "norm_ppf", "a": [p], "v": float(ss.norm.ppf(p))})
    for a, b, x in [(0.5, 0.5, 0.3), (2, 3, 0.4), (10, 20, 0.2), (150, 0.5, 0.98), (0.5, 60, 0.01)]:
        spc.append({"f": "beta_inc", "a": [a, b, x], "v": float(sp.betainc(a, b, x))})
    for x, df in [(0.5, 1), (3.84, 1), (10, 3), (50, 10), (200, 150)]:
        spc.append({"f": "chi2_sf", "a": [x, df], "v": float(ss.chi2.sf(x, df))})
    for f, d1, d2 in [(1.0, 1, 10), (4.96, 1, 10), (30.0, 1, 300), (2.5, 3, 40)]:
        spc.append({"f": "f_sf", "a": [f, d1, d2], "v": float(ss.f.sf(f, d1, d2))})
    out["special"] = spc

    # ── describe ─────────────────────────────────────────────────────────
    v = np.round(rng.lognormal(3.0, 0.8, 237), 3)
    sv = np.sort(v)
    med = float(np.median(v))
    out["describe"] = {
        "values": list(map(float, v)),
        "mean": float(v.mean()),
        "sd": float(v.std(ddof=1)),
        "q": [q7(sv, p) for p in [0, 0.05, 0.25, 0.5, 0.75, 0.95, 1]],
        "mad": float(np.median(np.abs(v - med))),
        "skewness": float(ss.skew(v, bias=False)),
        "kurtosis": float(ss.kurtosis(v, bias=False)),
    }

    # ── correlation ──────────────────────────────────────────────────────
    ans_x = [10, 8, 13, 9, 11, 14, 6, 4, 12, 7, 5]
    ans = {
        "anscombe1": (ans_x, [8.04, 6.95, 7.58, 8.81, 8.33, 9.96, 7.24, 4.26, 10.84, 4.82, 5.68]),
        "anscombe2": (ans_x, [9.14, 8.14, 8.74, 8.77, 9.26, 8.10, 6.13, 3.10, 9.13, 7.26, 4.74]),
        "anscombe3": (ans_x, [7.46, 6.77, 12.74, 7.11, 7.81, 8.84, 6.08, 5.39, 8.15, 6.42, 5.73]),
        "anscombe4": ([8, 8, 8, 8, 8, 8, 8, 19, 8, 8, 8], [6.58, 5.76, 7.71, 8.84, 8.47, 7.04, 5.25, 12.50, 5.56, 7.91, 6.89]),
    }
    corr = [corr_block(k, *map(np.asarray, xy)) for k, xy in ans.items()]
    xs = rng.normal(0, 1, 339)
    ys = 0.4 * xs + rng.normal(0, 1, 339)
    xs_t = np.round(xs, 1)  # ties
    corr.append(corr_block("synthetic339_ties", xs_t, np.round(ys, 1)))
    out["correlate"] = corr

    # ── regression ───────────────────────────────────────────────────────
    st = sm.datasets.stackloss.load_pandas().data
    lg = sm.datasets.longley.load_pandas().data
    reg = [
        ols_block("stackloss", st.STACKLOSS.values, [st.AIRFLOW.values, st.WATERTEMP.values, st.ACIDCONC.values],
                  ["const", "AIRFLOW", "WATERTEMP", "ACIDCONC"]),
        ols_block("longley", lg.TOTEMP.values, [lg[c].values for c in ["GNPDEFL", "GNP", "UNEMP", "ARMED", "POP", "YEAR"]],
                  ["const", "GNPDEFL", "GNP", "UNEMP", "ARMED", "POP", "YEAR"]),
        ols_block("anscombe1", np.asarray(ans["anscombe1"][1]), [np.asarray(ans_x, float)], ["const", "x"]),
    ]
    n = 400
    x1 = rng.uniform(0, 60, n)
    x2 = rng.normal(5, 2, n)
    y = 3 - 0.02 * x1 + 0.3 * x2 + rng.standard_t(3, n) * 0.4
    reg.append(ols_block("synthetic400_t3", y, [x1, x2], ["const", "x1", "x2"]))
    out["regress"] = reg

    # ── comparisons ──────────────────────────────────────────────────────
    a = np.round(rng.normal(10, 3, 180), 1)
    b = np.round(rng.normal(11.2, 4, 95), 1)
    w = ss.ttest_ind(a, b, equal_var=False)
    wci = w.confidence_interval(0.95)
    mw = ss.mannwhitneyu(a, b, alternative="two-sided", method="asymptotic", use_continuity=True)
    na, nb = len(a), len(b)
    sp_ = math.sqrt(((na - 1) * a.var(ddof=1) + (nb - 1) * b.var(ddof=1)) / (na + nb - 2))
    out["compare"] = {
        "a": list(map(float, a)),
        "b": list(map(float, b)),
        "welch_t": float(w.statistic),
        "welch_df": float(w.df),
        "welch_p": float(w.pvalue),
        "welch_ci": [float(wci.low), float(wci.high)],
        "mw_u": float(mw.statistic),
        "mw_p": float(mw.pvalue),
        "hodges_lehmann": float(np.median(np.subtract.outer(a, b))),
        "cohens_d": float((a.mean() - b.mean()) / sp_),
        "bootstrap": {
            "seed": 6782119904115,
            "stream": 3,
            "reps": 200,
            "level": 0.95,
            "median_diff": bits(bootstrap([float(v) for v in a], [float(v) for v in b], "median", 200, 0.95, 6782119904115, 3)),
            "mean_one": bits(bootstrap([float(v) for v in a], None, "mean", 200, 0.9, 6782119904115, 4)),
        },
    }

    # ── classification ───────────────────────────────────────────────────
    cv = list(map(float, np.round(rng.gamma(2.0, 10.0, 150), 2)))
    scv = sorted(cv)
    out["classify"] = {
        "values": cv,
        "k": 5,
        "quantile": [q7(scv, i / 5) for i in range(1, 5)],
        "equal": [scv[0] + (scv[-1] - scv[0]) * i / 5 for i in range(1, 5)],
        "jenks": jenks(cv, 5),
        "headtail": headtail(cv, 5),
        "stddev": stddev_breaks(cv, 5),
    }

    # ── effective n ──────────────────────────────────────────────────────
    m = 90
    lon = rng.uniform(79.7, 81.8, m)
    lat = rng.uniform(5.9, 9.8, m)
    ex = np.sin(lat * 2) + 0.3 * rng.normal(size=m)
    ey = np.cos(lon * 3) + np.sin(lat * 2) * 0.5 + 0.3 * rng.normal(size=m)
    out["effn"] = {
        "x": list(map(float, ex)),
        "y": list(map(float, ey)),
        "lon": list(map(float, lon)),
        "lat": list(map(float, lat)),
        "m": effn(ex, ey, lon, lat),
    }
    # ── Moran's I, kNN weights, seeded permutations ─────────────────────
    c = math.cos(float(np.mean(lat)) * math.pi / 180)
    kk = 6
    nb = []
    for i in range(m):
        d = sorted(((float((lon[j] * c - lon[i] * c) ** 2 + (lat[j] - lat[i]) ** 2), j) for j in range(m) if j != i))
        nb.append([j for _, j in d[:kk]])
    resid = [float(v) for v in ex]
    mean_r = 0.0
    for v in resid:
        mean_r += v
    mean_r /= m
    z = [v - mean_r for v in resid]

    def moran(zz):
        num = 0.0
        for i in range(m):
            lag = 0.0
            for j in nb[i]:
                lag += zz[j]
            num += zz[i] * lag * (1.0 / kk)
        den = 0.0
        for v in zz:
            den += v * v
        return num / den

    i_obs = moran(z)
    s_ = Stream(424242, 9)
    ge = 0
    zz = list(z)
    for _ in range(499):
        for a_ in range(m - 1, 0, -1):
            b_ = s_.below(a_ + 1)
            zz[a_], zz[b_] = zz[b_], zz[a_]
        if moran(zz) >= i_obs:
            ge += 1
    out["moran"] = {
        "values": resid, "lon": list(map(float, lon)), "lat": list(map(float, lat)), "k": kk,
        "neighbours": nb, "i": i_obs, "i_bits": bits([i_obs])[0],
        "seed": 424242, "stream": 9, "permutations": 499, "p_perm": (ge + 1) / 500,
    }
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
