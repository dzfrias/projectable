use criterion::{criterion_group, criterion_main, Criterion};
use projectable::filelisting::FileListing;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut listing = FileListing::new(&[
        "/root/Cargo.lock",
        "/root/Cargo.toml",
        "/root/LICENSE",
        "/root/README.md",
        "/root/benches/listing_selection.rs",
        "/root/extras/CONFIG.md",
        "/root/extras/screenshot.png",
        "/root/src/app/component.rs",
        "/root/src/app/components/file_cmd_popup.rs",
        "/root/src/app/components/filetree.rs",
        "/root/src/app/components/fuzzy_match.rs",
        "/root/src/app/components/input_box.rs",
        "/root/src/app/components/marks_popup.rs",
        "/root/src/app/components/mod.rs",
        "/root/src/app/components/pending_popup.rs",
        "/root/src/app/components/popup.rs",
        "/root/src/app/components/preview_file.rs",
        "/root/src/app/components/testing.rs",
        "/root/src/app/mod.rs",
        "/root/src/config.rs",
        "/root/src/config_defaults/",
        "/root/src/config_defaults/unix.toml",
        "/root/src/config_defaults/windows.toml",
        "/root/src/external_event/",
        "/root/src/external_event/crossterm_event.rs",
        "/root/src/external_event/mod.rs",
        "/root/src/external_event/refresh.rs",
        "/root/src/external_event/run_cmd.rs",
        "/root/src/filelisting/items.rs",
        "/root/src/filelisting/listing.rs",
        "/root/src/filelisting/mod.rs",
        "/root/src/lib.rs",
        "/root/src/main.rs",
        "/root/src/marks.rs",
        "/root/src/queue.rs",
        "/root/src/ui/mod.rs",
        "/root/src/ui/scroll_paragraph.rs",
    ]);
    c.bench_function("next item in listing", |b| {
        b.iter(|| {
            listing.fold("/root/benches");
            listing.select_next();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
