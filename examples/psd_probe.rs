use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: psd_probe <file.psd>"),
    );
    let document = xuan::io::import_psd(&path)?;
    println!(
        "canvas {}x{} layers {}",
        document.width,
        document.height,
        document.layers.len()
    );
    for layer in &document.layers {
        println!(
            "- {:?} visible={} opacity={:.3} blend={:?} at ({:.0}, {:.0}) size {:.0}x{:.0} clip={:?}",
            layer.name,
            layer.visible,
            layer.opacity,
            layer.blend,
            layer.transform.x,
            layer.transform.y,
            layer.transform.width,
            layer.transform.height,
            layer.clip_to
        );
    }
    let flat = xuan::io::import_psd_image(&path)?;
    println!("flattened {}x{}", flat.width(), flat.height());
    Ok(())
}
