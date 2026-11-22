//! The registry's PDF table templates against the real publications.
//!
//! Gated: set `LANKA_PDF_FIXTURES` to a directory holding the files named
//! by the connectors' `discover.files[].key` (`<key>.pdf`) — the same
//! directory `lanka ingest run <source> --from DIR` reads. Each available
//! table must be located, read with bbox locators, and pass every template
//! check; a few printed values are pinned.

use std::path::PathBuf;

use lanka_extractd::Sandbox;
use lanka_ingest::template::{self, Value};

fn registry() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry")
}

/// (file key, edition, template id, rows, (row, column, printed value))
#[allow(clippy::type_complexity)]
const CASES: &[(&str, &str, &str, usize, (&str, &str, &str))] = &[
    (
        "abstract-2025-2.1a",
        "2025",
        "dcs-statistical-abstract/table-2.1a-midyear-population-district",
        26,
        ("Nuwara - Eliya", "total_y5", "779"),
    ),
    (
        "lfs-annual-2025",
        "2025",
        "dcs-lfs/annual-table-4.1-employment-by-industry",
        7,
        ("2025", "services_no", "4,026,037"),
    ),
    (
        "debt-bulletin-2026-q2",
        "2026-Q2",
        "treasury-debt/bulletin-outstanding-public-debt",
        15,
        ("Market Borrowings", "usd_curr", "12,409"),
    ),
];

#[tokio::test]
async fn real_tables_read_and_pass_their_checks() {
    let Some(dir) = std::env::var_os("LANKA_PDF_FIXTURES").map(PathBuf::from) else {
        eprintln!("SKIPPED real_tables_read_and_pass_their_checks: LANKA_PDF_FIXTURES not set");
        return;
    };
    let sandbox = Sandbox::local();
    let mut ran = 0;
    for (key, edition, id, n_rows, (row, col, printed)) in CASES {
        let path = dir.join(format!("{key}.pdf"));
        let Ok(pdf) = std::fs::read(&path) else {
            eprintln!("skipping {id}: {} missing", path.display());
            continue;
        };
        let (tpl, _) = template::load(&registry(), id).expect("template");
        let words = sandbox.pdf_words(&pdf, 0, 0).await.expect("words");
        let table = tpl.read(&words, Some(edition)).expect("read");
        let (_, ed) = tpl.edition(Some(edition)).expect("edition");
        let checks = table.check(ed);
        assert!(
            checks.iter().all(|c| c.ok),
            "{id}: {:?}",
            template::failed_summary(&checks)
        );
        assert_eq!(table.rows.len(), *n_rows, "{id}");
        let r = table.row(row).unwrap_or_else(|| panic!("{id}: row {row}"));
        let cell = &table.rows[r].cells[*col];
        assert_eq!(cell.text, *printed, "{id}");
        assert!(matches!(cell.value, Value::Number(_)));
        assert!(cell.page >= 1 && cell.bbox[2] > cell.bbox[0] && cell.bbox[3] > cell.bbox[1]);
        // Every cell carries a locator.
        assert!(
            table
                .rows
                .iter()
                .all(|r| r.cells.values().all(|c| c.bbox != [0.0; 4]))
        );
        ran += 1;
    }
    eprintln!("{ran} real table(s) checked");
}
