fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(path)?;
    let psd = psd::Psd::from_bytes(&bytes).unwrap();
    println!("canvas {}x{}", psd.width(), psd.height());
    for (i, l) in psd.layers().iter().enumerate() {
        println!(
            "{i}: {:?} clip={} visible={} opacity={} blend={:?} left={} top={} right={} bottom={}",
            l.name(),
            l.is_clipping_mask(),
            l.visible(),
            l.opacity(),
            l.blend_mode(),
            l.layer_left(),
            l.layer_top(),
            l.layer_right(),
            l.layer_bottom()
        );
    }
    Ok(())
}
