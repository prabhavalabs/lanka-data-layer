#!/usr/bin/env python3
"""Reference fixture for partial correlation (lanka-stats correlate::partial).

numpy + scipy only (the formulas of pingouin.partial_corr written out):
residuals of x and y on [1, controls] by least squares, Pearson r of the
residuals, t on n − 2 − k df, Fisher-z interval with √(n − 3 − k); the
Spearman form ranks every column first (interval variance 1.06). The
effective-n test is F = (M − 1 − k)·r²/(1 − r²) on (1, M − 2 − k).
Run from this directory:  python3 -I gen_partial.py > partial.json
"""

import json

import numpy as np
import scipy.stats as ss


def resid(v, C):
    X = np.column_stack([np.ones(len(v))] + C)
    b, *_ = np.linalg.lstsq(X, v, rcond=None)
    return v - X @ b


def partial(x, y, C, rank):
    if rank:
        x, y = ss.rankdata(x), ss.rankdata(y)
        C = [ss.rankdata(c) for c in C]
    ex, ey = resid(x, C), resid(y, C)
    r = float(np.corrcoef(ex, ey)[0, 1])
    n, k = len(x), len(C)
    df = n - 2 - k
    t = r * np.sqrt(df / (1 - r * r))
    p = float(2 * ss.t.sf(abs(t), df))
    se = np.sqrt((1.06 if rank else 1.0) / (n - 3 - k))
    q = ss.norm.ppf(0.975)
    z = np.arctanh(r)
    return {"r": r, "p": p, "ci": [float(np.tanh(z - q * se)), float(np.tanh(z + q * se))]}


def main():
    rng = np.random.default_rng(20261122)
    n = 60
    z1 = rng.normal(size=n)
    z2 = rng.uniform(size=n) * 3
    x = 0.8 * z1 + 0.3 * z2 + rng.normal(scale=0.6, size=n)
    y = -0.5 * z1 + 0.9 * z2 + 0.25 * x + rng.normal(scale=0.7, size=n)
    out = {"x": x.tolist(), "y": y.tolist(), "controls": [z1.tolist(), z2.tolist()], "cases": []}
    for k in (1, 2):
        C = [z1, z2][:k]
        out["cases"].append({"k": k, "pearson": partial(x, y, C, False), "spearman": partial(x, y, C, True)})
    out["p_adjusted_k"] = [
        {"r": r, "m": m, "k": k, "p": float(ss.f.sf((m - 1 - k) * r * r / (1 - r * r), 1, m - 2 - k))}
        for r, m, k in [(0.31, 84.0, 0), (0.31, 84.0, 2), (-0.12, 25.5, 1), (0.6, 9.0, 3)]
    ]
    json.dump(out, __import__("sys").stdout, indent=1)


main()
