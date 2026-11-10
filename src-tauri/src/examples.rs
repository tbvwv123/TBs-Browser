//! 처음 실행할 때 사용자의 스크립트 폴더에 넣는 예시 (spec 6.5). 원본: scripts-examples/

pub const FILES: &[(&str, &[u8])] = &[
    ("chess-pieces/script.json", include_bytes!("../../scripts-examples/chess-pieces/script.json")),
    ("chess-pieces/style.css", include_bytes!("../../scripts-examples/chess-pieces/style.css")),
    ("chess-pieces/pieces/bb.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/bb.svg")),
    ("chess-pieces/pieces/bk.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/bk.svg")),
    ("chess-pieces/pieces/bn.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/bn.svg")),
    ("chess-pieces/pieces/bp.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/bp.svg")),
    ("chess-pieces/pieces/bq.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/bq.svg")),
    ("chess-pieces/pieces/br.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/br.svg")),
    ("chess-pieces/pieces/wb.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wb.svg")),
    ("chess-pieces/pieces/wk.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wk.svg")),
    ("chess-pieces/pieces/wn.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wn.svg")),
    ("chess-pieces/pieces/wp.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wp.svg")),
    ("chess-pieces/pieces/wq.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wq.svg")),
    ("chess-pieces/pieces/wr.svg", include_bytes!("../../scripts-examples/chess-pieces/pieces/wr.svg")),
    ("youtube-screenshot/main.js", include_bytes!("../../scripts-examples/youtube-screenshot/main.js")),
    ("youtube-screenshot/script.json", include_bytes!("../../scripts-examples/youtube-screenshot/script.json")),
    ("youtube-screenshot/style.css", include_bytes!("../../scripts-examples/youtube-screenshot/style.css")),
];
