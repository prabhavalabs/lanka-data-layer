//! The global recipe DAG (04 §7): recipe B depends on recipe A when one of
//! B's inputs is A's output dataset. Kahn's algorithm with ties broken by
//! recipe id, so the order is deterministic.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::recipe::RecipeSpec;

/// Topological order of `recipes` (indices into the slice).
pub fn order(recipes: &[&RecipeSpec]) -> Result<Vec<usize>> {
    let producer: BTreeMap<&str, usize> = recipes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.output.as_str(), i))
        .collect();
    let mut indeg = vec![0usize; recipes.len()];
    let mut down: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); recipes.len()];
    for (i, r) in recipes.iter().enumerate() {
        let ups: BTreeSet<usize> = r
            .inputs
            .values()
            .filter_map(|inp| producer.get(inp.dataset.as_str()).copied())
            .collect();
        for u in ups {
            if down[u].insert(i) {
                indeg[i] += 1;
            }
        }
    }
    let mut ready: BTreeSet<(&str, usize)> = recipes
        .iter()
        .enumerate()
        .filter(|(i, _)| indeg[*i] == 0)
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut out = Vec::with_capacity(recipes.len());
    while let Some(first) = ready.pop_first() {
        let i = first.1;
        out.push(i);
        for &d in &down[i] {
            indeg[d] -= 1;
            if indeg[d] == 0 {
                ready.insert((recipes[d].id.as_str(), d));
            }
        }
    }
    if out.len() != recipes.len() {
        let stuck: Vec<&str> = recipes
            .iter()
            .enumerate()
            .filter(|(i, _)| indeg[*i] > 0)
            .map(|(_, r)| r.id.as_str())
            .collect();
        bail!("recipe DAG has a cycle through: {}", stuck.join(", "));
    }
    Ok(out)
}

/// Recipes downstream of `id` (transitively), excluding `id`.
#[must_use]
pub fn downstream(recipes: &[&RecipeSpec], id: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut frontier: Vec<String> = recipes
        .iter()
        .filter(|r| r.id == id)
        .map(|r| r.output.clone())
        .collect();
    while let Some(ds) = frontier.pop() {
        for r in recipes {
            if r.inputs.values().any(|i| i.dataset == ds) && out.insert(r.id.clone()) {
                frontier.push(r.output.clone());
            }
        }
    }
    out
}
