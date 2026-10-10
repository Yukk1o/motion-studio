use motion_media::fonts::{FontStore, BUILTIN_BYTES, GLYPH_CACHE_BYTES};

#[test]
fn fonts_share_glyph_cache_for_atlas_and_text_and_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let id = FontStore::builtin_id();
    let mut fonts = FontStore::new(root.path()).unwrap();
    let atlas = fonts.ascii_atlas(&id, "@ .#", 32.).unwrap();
    assert_eq!(atlas.characters.chars().count(), 4);
    assert!(atlas.coverage.windows(2).all(|w| w[0] <= w[1]));
    assert!(atlas.coverage[0] == 0.);
    assert_eq!(atlas.raster.width, atlas.cell[0] * 4);
    assert_eq!(
        atlas.raster.rgba.len(),
        (atlas.raster.width * atlas.raster.height * 4) as usize
    );
    let before = fonts.cache_hits;
    let text = fonts.raster_text(&id, "@ .#\n@", 32., 100).unwrap();
    assert!(fonts.cache_hits > before);
    assert!(text.height > atlas.cell[1]);
    assert!(text.rgba.chunks_exact(4).any(|p| p[3] > 0));
    assert!(fonts.cache_bytes() <= GLYPH_CACHE_BYTES);
    let again = fonts.ascii_atlas(&id, "@ .#", 32.).unwrap();
    assert_eq!(atlas.raster.rgba, again.raster.rgba);
    drop(fonts);
    let mut reopened = FontStore::new(root.path()).unwrap();
    assert_eq!(
        reopened.ascii_atlas(&id, "@ .#", 32.).unwrap().raster.rgba,
        atlas.raster.rgba
    );
}

#[test]
fn malformed_fonts_glyph_requests_and_atlas_overflow_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut fonts = FontStore::new(root.path()).unwrap();
    let id = FontStore::builtin_id();
    assert!(fonts.import_bytes(b"not a font", "").is_err());
    assert!(fonts.import_face_bytes(BUILTIN_BYTES, "", 255).is_err());
    assert!(fonts.glyph(&id, 'A', f32::NAN).is_err());
    assert!(fonts.glyph("../escape", 'A', 16.).is_err());
    assert!(fonts.ascii_atlas(&id, "AA", 16.).is_err());
    assert!(fonts.ascii_atlas(&id, "A\n", 16.).is_err());
    assert!(fonts.raster_text(&id, "x", 32., 8193).is_err());
}

#[test]
fn project_packages_retain_font_bytes_and_license() {
    let root = tempfile::tempdir().unwrap();
    let fonts_dir = root.path().join("assets/fonts");
    let mut fonts = FontStore::new(&fonts_dir).unwrap();
    let info = fonts
        .import_bytes(BUILTIN_BYTES, "test license notice")
        .unwrap();
    let mut project = motion_core::Project::new(64, 64, 30, 30).unwrap();
    project.fonts.push(motion_core::FontAsset {
        id: info.id.clone(),
        path: format!("assets/fonts/{}.ttf", info.id),
        name: info.name,
        face_index: 0,
        license: info.license,
    });
    motion_core::storage::save(root.path(), &project).unwrap();
    let package = root.path().join("fonts.msproj");
    motion_core::storage::export_package(root.path(), &project, &package).unwrap();
    let restored = root.path().join("restored");
    let loaded = motion_core::storage::import_package(&package, &restored).unwrap();
    assert_eq!(loaded.fonts, project.fonts);
    assert_eq!(
        std::fs::read(restored.join(&loaded.fonts[0].path)).unwrap(),
        BUILTIN_BYTES
    );
}
