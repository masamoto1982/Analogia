![Rust](docs/assets/badges/rust.svg) ![WebAssembly](docs/assets/badges/webassembly.svg) ![TypeScript](docs/assets/badges/typescript.svg) ![Tauri](docs/assets/badges/tauri.svg)

# Analogia

MONIAC（フィリップス機）のような流体式アナログコンピュータを、テキストで組み立てて動かすプログラミング言語です。タンク（ストック）に量が貯まり、パイプ（フロー）がそれを流し、`RUN` で連続時間のまま時間を進めます。パイプは量を移すだけなので、外界につながらない限り全体の量は保存されます。

```
100 'BATH' TANK                                  # 水位 100 のタンク
[ 5 ] 'OUTSIDE' 'BATH' 'TAP' PIPE                # 外から毎時 5 流れ込む
[ BATH 0.1 MUL ] 'BATH' 'OUTSIDE' 'DRAIN' PIPE   # 水位の 1 割が流れ出る
30 0.1 RUN                                       # 30 時間流す（RK4）
```

```
RUN t = 0 → 30  (dt 0.1, 300 steps)
  BATH         100 ██▇▆▆▆▅▅▅▄▄▄▄▃▃▃▃▃▂▂▂▂▂▂▂▂▂▂▁▁▁▁▁▁▁▁▁▁▁▁ 52.49
  flows at t = 30
  TAP    OUTSIDE → BATH          5
  DRAIN  BATH → OUTSIDE       5.25
  total in tanks  100 → 52.49
```

言語の設計は [`docs/design.md`](docs/design.md)、MONIAC 風の国民経済モデルは [`examples/moniac.alg`](examples/moniac.alg) にあります。Rust のコアを WebAssembly にして、TypeScript の Playground（ブラウザ、または Tauri のデスクトップアプリ）から動かします。

## 構成

| 場所 | 役割 |
|---|---|
| `rust/` | コア。`src/interpreter.rs` が言語（ワードと RK4 シミュレータ）、`src/model.rs` がタンクとパイプ、`src/wasm.rs` が Playground とのやり取り（`wasm` feature で有効） |
| `examples/` | サンプルプログラム（`moniac.alg`） |
| `docs/design.md` | 言語の設計メモ |
| `src/wasm/generated/` | `npm run build:wasm` が書き出す wasm バンドル（コミット対象） |
| `src/wasm-interpreter-types.ts` | Rust と TypeScript の間の約束（インタープリタのメソッドと値の形） |
| `src/workers/` | wasm を Web Worker で実行する仕組み。重い処理や無限ループでも GUI が固まらず、タイムアウトと中断ができる |
| `src/gui/` | Playground の GUI（エディタ、出力、スタック、辞書、ステップ実行、保存と復元、レイアウト） |
| `src/platform/` | Web と Tauri（デスクトップ）の違いの吸収、cross-origin isolation |
| `src/styles/playground.css` | 配色とレイアウト。`npm run check:contrast` が WCAG のコントラストを検査 |
| `public/docs/` | Playground から開くリファレンス（日英）。例は「Playground で開く」でエディタに読み込める |
| `src-tauri/` | デスクトップアプリ（Tauri） |
| `docs/dev/gui-current-design-memory.md` | GUI の設計メモ |

## ビルドと実行

| 作業 | コマンド |
|---|---|
| 依存関係のインストール | `npm ci` |
| 開発サーバー | `npm run dev` |
| wasm のビルド | `npm run build:wasm`（初回は `bash ./scripts/install-wasm-pack.sh`） |
| ブラウザ向けビルド | `npm run build` |
| デスクトップアプリのビルド | `npm run tauri:build` |
| TypeScript のテスト | `npm test` |
| Rust のテスト | `cargo test`（`rust/` で実行） |

`main` への push で GitHub Pages にデプロイされます（`.github/workflows/build.yml`）。

## ライセンス

MIT（[`LICENSE`](LICENSE)）。同梱しているサードパーティ製品は [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md) を参照してください。
