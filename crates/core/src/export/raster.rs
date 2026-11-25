//! PNG export of a diagram. Same [`super::Graph`] as the SVG emitter, drawn
//! into an RGBA buffer and wrapped in a stored-deflate PNG. No image crate.
use super::util::{self, Graph};
use super::Diagram;
use crate::analysis::Analysis;
use crate::model::Index;

const CELL_W: usize = 168;
const CELL_H: usize = 44;
const MARGIN: usize = 16;

/// PNG bytes for `diagram` (call graph, modules, or any other export diagram).
pub fn png(index: &Index, analysis: &Analysis, diagram: &Diagram) -> Vec<u8> {
    let (graph, _title, cap) = match diagram {
        Diagram::CallGraph => (
            util::build_call_graph(index, util::SVG_NODE_CAP),
            "Call graph",
            util::SVG_NODE_CAP,
        ),
        Diagram::Modules => (util::build_modules(analysis), "Modules", util::SVG_NODE_CAP),
        Diagram::Types => (util::build_types(analysis), "Types", util::SVG_NODE_CAP),
        Diagram::Cfg { symbol } => (
            util::build_cfg(analysis, symbol),
            "Control flow",
            util::SVG_NODE_CAP,
        ),
        Diagram::Directory { imports } => (
            super::directory::build(
                index,
                analysis,
                &super::directory::DirectoryOptions {
                    import_edges: *imports,
                    loc_labels: true,
                },
            ),
            "Directory",
            util::SVG_NODE_CAP,
        ),
    };
    let _ = cap;
    encode(&paint(&graph))
}

struct Image {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

fn paint(graph: &Graph) -> Image {
    let n = graph.nodes.len().max(1);
    let cols = ((n as f64).sqrt().ceil() as usize).clamp(1, 8);
    let rows = n.div_ceil(cols);
    let w = MARGIN * 2 + cols * CELL_W;
    let h = MARGIN * 2 + rows * CELL_H + 18;
    let mut img = Image {
        w,
        h,
        px: vec![[0x18, 0x1b, 0x21, 0xff]; w * h],
    };
    let mut at: Vec<(usize, usize)> = Vec::with_capacity(graph.nodes.len());
    for (i, node) in graph.nodes.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = MARGIN + col * CELL_W + 8;
        let y = MARGIN + row * CELL_H + 8;
        at.push((x + 70, y + 14));
        fill_rect(&mut img, x, y, 148, 28, [0x2a, 0x30, 0x3a, 0xff]);
        draw_text(
            &mut img,
            x + 6,
            y + 10,
            &node.label,
            [0xe6, 0xe9, 0xef, 0xff],
        );
    }
    let mut id_at = std::collections::BTreeMap::new();
    for (node, pos) in graph.nodes.iter().zip(at) {
        id_at.insert(node.id.as_str(), pos);
    }
    for edge in &graph.edges {
        if let (Some(&a), Some(&b)) = (id_at.get(edge.from.as_str()), id_at.get(edge.to.as_str())) {
            line(&mut img, a, b, [0x4c, 0x8d, 0xff, 0xff]);
        }
    }
    if let Some(caption) = &graph.caption {
        draw_text(
            &mut img,
            MARGIN,
            h.saturating_sub(14),
            caption,
            [0xe3, 0xb3, 0x41, 0xff],
        );
    }
    img
}

fn fill_rect(img: &mut Image, x: usize, y: usize, w: usize, h: usize, color: [u8; 4]) {
    for yy in y..y.saturating_add(h).min(img.h) {
        for xx in x..x.saturating_add(w).min(img.w) {
            img.px[yy * img.w + xx] = color;
        }
    }
}

fn line(img: &mut Image, a: (usize, usize), b: (usize, usize), color: [u8; 4]) {
    let (mut x0, mut y0) = (a.0 as i32, a.1 as i32);
    let (x1, y1) = (b.0 as i32, b.1 as i32);
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        if x0 >= 0 && y0 >= 0 && (x0 as usize) < img.w && (y0 as usize) < img.h {
            img.px[y0 as usize * img.w + x0 as usize] = color;
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_text(img: &mut Image, x: usize, y: usize, text: &str, color: [u8; 4]) {
    let mut cursor = x;
    for ch in text.chars().take(22) {
        blit(img, cursor, y, glyph(ch), color);
        cursor += 6;
        if cursor + 5 >= img.w {
            break;
        }
    }
}

fn blit(img: &mut Image, x: usize, y: usize, rows: [u8; 7], color: [u8; 4]) {
    for (ry, row) in rows.iter().enumerate() {
        for bit in 0..5 {
            if row & (1 << (4 - bit)) != 0 {
                let xx = x + bit;
                let yy = y + ry;
                if xx < img.w && yy < img.h {
                    img.px[yy * img.w + xx] = color;
                }
            }
        }
    }
}

fn glyph(ch: char) -> [u8; 7] {
    let c = match ch {
        'A'..='Z' => ch.to_ascii_lowercase(),
        other => other,
    };
    match c {
        'a' => [0b01110, 0b00001, 0b01111, 0b10001, 0b01111, 0, 0],
        'b' => [0b10000, 0b10000, 0b11110, 0b10001, 0b11110, 0, 0],
        'c' => [0b01110, 0b10000, 0b10000, 0b10000, 0b01110, 0, 0],
        'd' => [0b00001, 0b00001, 0b01111, 0b10001, 0b01111, 0, 0],
        'e' => [0b01110, 0b10001, 0b11111, 0b10000, 0b01110, 0, 0],
        'f' => [0b00110, 0b01000, 0b11100, 0b01000, 0b01000, 0, 0],
        'g' => [0b01111, 0b10001, 0b01111, 0b00001, 0b01110, 0, 0],
        'h' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0, 0],
        'i' => [0b00100, 0, 0b01100, 0b00100, 0b01110, 0, 0],
        'j' => [0b00010, 0, 0b00010, 0b00010, 0b01100, 0, 0],
        'k' => [0b10000, 0b10010, 0b11100, 0b10010, 0b10001, 0, 0],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b01110, 0, 0],
        'm' => [0b11010, 0b10101, 0b10101, 0b10101, 0b10101, 0, 0],
        'n' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0, 0],
        'o' => [0b01110, 0b10001, 0b10001, 0b10001, 0b01110, 0, 0],
        'p' => [0b11110, 0b10001, 0b11110, 0b10000, 0b10000, 0, 0],
        'q' => [0b01111, 0b10001, 0b01111, 0b00001, 0b00001, 0, 0],
        'r' => [0b10110, 0b11000, 0b10000, 0b10000, 0b10000, 0, 0],
        's' => [0b01111, 0b10000, 0b01110, 0b00001, 0b11110, 0, 0],
        't' => [0b01000, 0b11100, 0b01000, 0b01000, 0b00110, 0, 0],
        'u' => [0b10001, 0b10001, 0b10001, 0b10011, 0b01101, 0, 0],
        'v' => [0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0, 0],
        'w' => [0b10001, 0b10001, 0b10101, 0b10101, 0b01010, 0, 0],
        'x' => [0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0, 0],
        'y' => [0b10001, 0b10001, 0b01111, 0b00001, 0b01110, 0, 0],
        'z' => [0b11111, 0b00010, 0b00100, 0b01000, 0b11111, 0, 0],
        '0' => [0b01110, 0b10011, 0b10101, 0b11001, 0b01110, 0, 0],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b01110, 0, 0],
        '2' => [0b01110, 0b00001, 0b00110, 0b01000, 0b11111, 0, 0],
        '3' => [0b11110, 0b00001, 0b00110, 0b00001, 0b11110, 0, 0],
        '4' => [0b00010, 0b00110, 0b01010, 0b11111, 0b00010, 0, 0],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b11110, 0, 0],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b01110, 0, 0],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0, 0],
        '8' => [0b01110, 0b10001, 0b01110, 0b10001, 0b01110, 0, 0],
        '9' => [0b01110, 0b10001, 0b01111, 0b00001, 0b01110, 0, 0],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0],
        '.' => [0, 0, 0, 0, 0b00100, 0, 0],
        '_' => [0, 0, 0, 0, 0b11111, 0, 0],
        '-' => [0, 0, 0b01110, 0, 0, 0, 0],
        '(' | ')' => [0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0, 0],
        ' ' => [0; 7],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b11111, 0, 0],
    }
}

fn encode(img: &Image) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(img.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let mut raw = Vec::with_capacity(img.h * (img.w * 4 + 1));
    for y in 0..img.h {
        raw.push(0);
        for x in 0..img.w {
            raw.extend_from_slice(&img.px[y * img.w + x]);
        }
    }
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let pieces: Vec<&[u8]> = if raw.is_empty() {
        vec![&[]]
    } else {
        raw.chunks(65535).collect()
    };
    for (i, chunk) in pieces.iter().enumerate() {
        let last = i + 1 == pieces.len();
        z.push(if last { 1 } else { 0 });
        let len = chunk.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for b in raw {
        s1 = (s1 + *b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    z.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    z
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Analysis;
    use crate::model::{Confidence, Edge, EdgeKind, Index, Symbol, SymbolKind};

    fn idx() -> Index {
        let mut index = Index::new("/repo");
        index.symbols.push(Symbol {
            id: "src/main.rs:1".into(),
            name: "main".into(),
            kind: SymbolKind::Function,
            signature: "fn main()".into(),
            doc: String::new(),
            start_line: 1,
            end_line: 4,
            container: None,
            file: "src/main.rs".into(),
        });
        index.symbols.push(Symbol {
            id: "src/main.rs:6".into(),
            name: "helper".into(),
            kind: SymbolKind::Function,
            signature: "fn helper()".into(),
            doc: String::new(),
            start_line: 6,
            end_line: 8,
            container: None,
            file: "src/main.rs".into(),
        });
        index.edges.push(Edge {
            caller: "src/main.rs:1".into(),
            callee: "src/main.rs:6".into(),
            line: 2,
            kind: EdgeKind::Call,
            confidence: Confidence::Exact,
        });
        index
    }

    #[test]
    fn call_graph_png_is_a_png_and_differs_from_an_empty_modules_png() {
        let bytes = png(&idx(), &Analysis::new(), &Diagram::CallGraph);
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        assert!(bytes.windows(4).any(|w| w == b"IHDR"));
        assert!(bytes.windows(4).any(|w| w == b"IEND"));
        let modules = png(&idx(), &Analysis::new(), &Diagram::Modules);
        assert_ne!(bytes, modules);
    }

    #[test]
    fn filtered_call_graph_png_drops_nodes_outside_the_path() {
        let mut full = idx();
        full.symbols.push(Symbol {
            id: "src/cli/main.rs:1".into(),
            name: "cli_main".into(),
            kind: SymbolKind::Function,
            signature: "fn cli_main()".into(),
            doc: String::new(),
            start_line: 1,
            end_line: 3,
            container: None,
            file: "src/cli/main.rs".into(),
        });
        full.edges.push(Edge {
            caller: "src/cli/main.rs:1".into(),
            callee: "src/main.rs:1".into(),
            line: 2,
            kind: EdgeKind::Call,
            confidence: Confidence::Exact,
        });
        let mut only_main = Index::new("/repo");
        only_main.symbols = full
            .symbols
            .iter()
            .filter(|s| s.file == "src/main.rs")
            .cloned()
            .collect();
        only_main.edges = full
            .edges
            .iter()
            .filter(|e| {
                only_main.symbols.iter().any(|s| s.id == e.caller)
                    && only_main.symbols.iter().any(|s| s.id == e.callee)
            })
            .cloned()
            .collect();
        let all = png(&full, &Analysis::new(), &Diagram::CallGraph);
        let filtered = png(&only_main, &Analysis::new(), &Diagram::CallGraph);
        assert_ne!(all, filtered);
        if let Some(dir) = std::env::var_os("REPOWALK_ARTIFACTS") {
            let dir = std::path::PathBuf::from(dir);
            let _ = std::fs::write(dir.join("graph-filter-search.png"), &filtered);
            let _ = std::fs::write(dir.join("graph-export.png"), &all);
            let _ = std::fs::write(
                dir.join("graph-export.svg"),
                crate::export::svg(&full, &Analysis::new(), &Diagram::CallGraph),
            );
        }
    }
}
