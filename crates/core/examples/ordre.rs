// SPDX-License-Identifier: GPL-3.0-or-later
fn main() -> anyhow::Result<()> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "rusty-music.db".into());
    let lib = rusty_music_core::db::Library::open(std::path::Path::new(&base))?;
    let t = std::time::Instant::now();
    let ordre = lib.ordre_darrivee()?;
    let mut par_source: std::collections::HashMap<&str, usize> = Default::default();
    for a in &ordre { *par_source.entry(a.source.as_str()).or_default() += 1; }
    let mut v: Vec<_> = par_source.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("{} arrivées en {:.2} s", ordre.len(), t.elapsed().as_secs_f64());
    for (s, n) in v { println!("  {s:<12} {n:>6}  {:>5.1} %", 100.0 * n as f64 / ordre.len() as f64); }
    println!("\npremière : {:?}", ordre.first().map(|a| (a.date, &a.source)));
    println!("dernière : {:?}", ordre.last().map(|a| (a.date, &a.source)));
    Ok(())
}
