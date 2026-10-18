use osaiku_ui::{compiler::compile, layout::Layout, Document, MAX_NODES, MAX_WIRE};

fn main() {
    let markup = include_str!("../hello.ui.html");
    let styles = include_str!("../hello.ui.css");
    let document = compile(markup, styles).expect("valid packaged view");
    let mut bytes = [0; MAX_WIRE];
    let size = document.encode(&mut bytes).expect("bounded encoding");
    let decoded = Document::decode(&bytes[..size]).expect("supported wire version");
    let layout = Layout::compute(&decoded, 480, 360, &[0; MAX_NODES]).expect("admitted viewport");
    for (index, n) in decoded.nodes[..decoded.len as usize].iter().enumerate() {
        println!(
            "{:08x} {:?} {:?} {}",
            n.id,
            n.kind,
            layout.nodes[index].bounds,
            decoded.text_of(index)
        );
    }
}
