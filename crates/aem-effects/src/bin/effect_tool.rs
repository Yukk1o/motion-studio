use aem_effects::{package_directory, EffectPackage};
use std::{env, fs, path::Path};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("builtin") if args.len()==3=>{let p=aem_effects::builtin::package()?;fs::write(&args[2],&p.bytes)?;println!("{} {} {}",p.manifest.id,p.manifest.version,p.hash);}
        Some("pack") if args.len()==4=>{let bytes=package_directory(Path::new(&args[2]))?;fs::write(&args[3],bytes)?;}
        Some("check") if args.len()==3=>{let p=EffectPackage::open(Path::new(&args[2]))?;println!("{} {} {} ({} effects)",p.manifest.id,p.manifest.version,p.hash,p.manifest.effects.len());}
        Some("glsl") if args.len()==4=>{let p=EffectPackage::open(Path::new(&args[2]))?;let out=Path::new(&args[3]);fs::create_dir_all(out)?;for ((id,index),shader) in &p.shaders {fs::write(out.join(format!("{id}-{index}.vert")),&shader.glsl.vertex)?;fs::write(out.join(format!("{id}-{index}.frag")),&shader.glsl.fragment)?;fs::write(out.join(format!("{id}-{index}.json")),serde_json::to_vec_pretty(&shader.glsl)?)?;}}
        _=>return Err("usage: effect_tool builtin <output.msfx> | pack <directory> <output.msfx> | check <package> | glsl <package> <directory>".into())
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
