use super::*;

fn png(side: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(side, side, |x, _| image::Rgba([x as u8, 0, 0, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn solid(side: u32) -> Arc<RenderImage> {
    render_image(RgbaImage::new(side, side))
}

#[test]
fn initials_take_two_words() {
    assert_eq!(initials("Dana Kim"), "DK");
    assert_eq!(initials("sam"), "S");
    assert_eq!(initials("Ana Maria de Souza"), "AM");
    assert_eq!(initials(""), "");
    // Policy groups: the `[Project]\` prefix isn't part of the name.
    assert_eq!(initials("[Contoso]\\React PR reviewers"), "RP");
    assert_eq!(initials("[Contoso]"), "[");
}

#[test]
fn decode_fits_large_pictures_and_keeps_small_ones() {
    // `size=2` (220 px) comes down to lg at 2×; the default 44 px stays.
    let large = decode(&png(220)).unwrap();
    assert_eq!(large.dimensions(), (DECODED_SIDE, DECODED_SIDE));
    let small = decode(&png(44)).unwrap();
    assert_eq!(small.dimensions(), (44, 44));
}

#[test]
fn decode_refuses_non_png_and_huge_images() {
    assert!(decode(b"<?xml version=\"1.0\"?><svg/>").is_none());
    assert!(decode(b"<html>").is_none());
    assert!(decode(&png(MAX_SOURCE_SIDE + 1)).is_none());
}

#[test]
fn decoded_cache_stays_within_its_bounds() {
    let mut state = State::new();
    for n in 0..1_000 {
        state.insert_ready(format!("p{n}").into(), solid(DECODED_SIDE));
    }
    assert!(state.ready <= MAX_ENTRIES);
    assert!(state.bytes <= MAX_BYTES);
    let ready = state
        .slots
        .values()
        .filter(|s| matches!(s, Slot::Ready { .. }))
        .count();
    assert_eq!(ready, state.ready);
    // The newest survive; the oldest went first.
    assert!(state.slots.contains_key("p999"));
    assert!(!state.slots.contains_key("p0"));
}

#[test]
fn eviction_spares_recently_drawn_pictures() {
    let mut state = State::new();
    state.insert_ready("keep".into(), solid(DECODED_SIDE));
    for n in 0..MAX_ENTRIES {
        // "keep" is on screen: drawn between every arrival.
        state.tick += 1;
        let tick = state.tick;
        if let Some(Slot::Ready { drawn, .. }) = state.slots.get_mut("keep") {
            *drawn = tick;
        }
        state.insert_ready(format!("p{n}").into(), solid(DECODED_SIDE));
    }
    assert!(state.slots.contains_key("keep"));
    assert!(!state.slots.contains_key("p0"));
}

#[test]
fn replacing_a_picture_does_not_double_count() {
    let mut state = State::new();
    state.insert_ready("a".into(), solid(DECODED_SIDE));
    state.insert_ready("a".into(), solid(DECODED_SIDE));
    assert_eq!(state.ready, 1);
    assert_eq!(state.bytes, (DECODED_SIDE * DECODED_SIDE * 4) as usize);
}

#[test]
fn generated_art_is_stable_opaque_and_per_person() {
    let a = art::generate("6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d", DECODED_SIDE);
    assert_eq!(
        a,
        art::generate("6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d", DECODED_SIDE)
    );
    assert!(a.pixels().all(|p| p.0[3] == 255));
    let b = art::generate("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", DECODED_SIDE);
    assert_ne!(a, b);
}

#[test]
fn demo_pictures_follow_the_payload_image_url() {
    let people = adoken_demo::dataset(7).people;
    let ids = demo_ids(&people);
    let with = people.iter().filter(|p| p.image_url.is_some()).count();
    let without = people.iter().filter(|p| p.image_url.is_none()).count();
    assert!(with > 0 && without > 0, "both paths are visible");
    assert!(ids.iter().all(|id| {
        people
            .iter()
            .any(|p| p.id == id.as_ref() && !p.is_group && p.image_url.is_some())
    }));
}
