//! CPU reference and captured commands from the actual SDK Painter.
use osaiku_runtime::gpu_painter::{Command, GpuPainter};
use osaiku_ui::{layout::Layout, paint::Scene, theme::Theme, Document};
use std::{collections::BTreeMap, io::Write, path::Path};
fn main() {
    let out = std::env::args().nth(1).expect("output");
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();
    let mut documents = Vec::new();
    documents.push(("primitives".to_string(),osaiku_ui::compiler::compile(
  "<main id=\"root\"><section id=\"card\"><p id=\"text\">Zażółć gęślą jaźń 0123456789</p><button id=\"go\" action=\"go\">Aplikacja</button><ui-icon id=\"icon\" src=\"test\"></ui-icon></section></main>",
  "main { width: 100%; padding: 7px; } section { width: 100%; padding: 5px; background-color: #24488880; border-radius: 11px; border-width: 3px; border-color: #e020807f; } p { font-size: 16px; } button { border-radius: 8px; }").unwrap()));
    for path in std::env::args().skip(2) {
        let b = std::fs::read(&path).unwrap();
        let mut d = Document::EMPTY;
        if Document::decode_into(&b, &mut d).is_ok() {
            documents.push((
                Path::new(&path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace('.', "-"),
                d,
            ));
        }
    }
    struct Series;
    impl osaiku_ui::Bindings for Series {
        fn get(&self,_:u32)->Option<osaiku_ui::Value<'_>> {
            static POINTS:[osaiku_ui::Point;5]=[
                osaiku_ui::Point{time_ms:0,value:0},osaiku_ui::Point{time_ms:1000,value:10000},
                osaiku_ui::Point{time_ms:2000,value:1200},osaiku_ui::Point{time_ms:3000,value:8500},
                osaiku_ui::Point{time_ms:4000,value:300}];
            Some(osaiku_ui::Value::Series(&POINTS))
        }
    }
    let chart=osaiku_ui::compiler::compile("<main id=\"root\"><ui-chart id=\"plot\" bind:series=\"values\"></ui-chart></main>","ui-chart { height: 180px; color: #123456; }").unwrap();
    let mut bound=Document::EMPTY;chart.bind_into(&Series,&mut bound).unwrap();documents.push(("chart-lines".into(),bound));
    static PIXELS: [u32; 16] = [
        0, 0x804080c0, 0xff102030, 0xffffffff, 0x40ff0000, 0x8000ff00, 0xc00000ff, 0xff000000,
        0xffffffff, 0x80808080, 0xffabcdef, 0xfffedcba, 0, 0xff00ffff, 0xffff00ff, 0xffffff00,
    ];
    const IMAGE: osaiku_ui::image::Image = osaiku_ui::image::Image {
        width: 4,
        height: 4,
        pixels: &PIXELS,
    };
    static IMAGES: [osaiku_ui::image::ImageSet; 1] = [osaiku_ui::image::ImageSet {
        name: "test",
        images: &[IMAGE],
    }];
    let mut count = 0;
    for (name, d) in &documents {
        for (theme_id, theme) in [Theme::Dark, Theme::Light].into_iter().enumerate() {
            for accent in [0, 3] {
                for (w,h) in [(384,256),(512,320)] {
                let mut layout = Layout::EMPTY;
                let scene = Scene::prepare(d, w, h, &mut layout)
                    .unwrap()
                    .with_theme(theme)
                    .with_accent(Some(accent))
                    .with_images(&IMAGES);
                let mut pixels = vec![0; w as usize * h as usize * 4];
                for (y, chunk) in pixels.chunks_mut(w as usize * 4 * 32).enumerate() {
                    osaiku_ui::raster::rows(&scene, (y * 32) as u32, chunk).unwrap();
                }
                let mut commands = Vec::new();
                let mut assets: Vec<Vec<u8>> = Vec::new();
                let mut ids = BTreeMap::new();
                let mut scratch = [Command::EMPTY; osaiku_runtime::gpu_painter::COMMAND_CAPACITY];
                let mut painter = GpuPainter::new(scene.layout().viewport, &mut scratch, |batch| {
                    assert_eq!(batch.op, 9);
                    let list = unsafe {
                        std::slice::from_raw_parts(
                            batch.data.cast::<Command>(),
                            batch.len / std::mem::size_of::<Command>(),
                        )
                    };
                    for c in list {
                        let mut c = *c;
                        if !c.data.is_null() {
                            let id = *ids.entry((c.data as usize, c.len)).or_insert_with(|| {
                                assets.push(
                                    unsafe { std::slice::from_raw_parts(c.data, c.len) }.to_vec(),
                                );
                                assets.len()
                            });
                            c.data = id as *const u8;
                        }
                        commands.push(c);
                    }
                    Ok(())
                });
                scene.paint(&mut painter);
                painter.finish().unwrap();
                let stem = format!("{count:03}-{name}-{theme_id}-{accent}");
                std::fs::write(out.join(format!("{stem}.bgra")), pixels).unwrap();
                let mut f = std::fs::File::create(out.join(format!("{stem}.cmd"))).unwrap();
                for n in [w, h, assets.len() as u32, commands.len() as u32] {
                    f.write_all(&n.to_le_bytes()).unwrap();
                }
                for asset in assets {
                    f.write_all(&(asset.len() as u32).to_le_bytes()).unwrap();
                    f.write_all(&asset).unwrap();
                }
                for c in commands {
                    f.write_all(unsafe {
                        std::slice::from_raw_parts(
                            (&c as *const Command).cast::<u8>(),
                            std::mem::size_of::<Command>(),
                        )
                    })
                    .unwrap();
                }
                count += 1;
                }
            }
        }
    }
    println!("generated {count} CPU/GPU Painter comparisons");
}
