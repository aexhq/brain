// StarlingMonkey compatibility rewrites for pi's dependency tree, applied while bundling.
// COMPAT (recorded in the H0 report): the pinned guest engine rejects Unicode property
// escapes in regex literals at parse time. typebox (a pi dependency) emits `\p{ID_Start}` in
// its identifier guard and builds IDN validators from `\p{…}` classes; the guard rewrites to
// an ASCII-equivalent class and the unused IDN modules stub out.
import { readFile } from "node:fs/promises";

const UNICODE_ID_REGEX_PATTERN = /\/\^\[\\p\{ID_Start\}[^/]*\*\$\/u/g;
const ASCII_ID_REGEX = String.raw`/^[A-Za-z_$][A-Za-z0-9_$]*$/`;

/** esbuild plugin for `buildLoopBundle({ plugins: [compatRewrite] })`. */
export const compatRewrite = {
  name: "starlingmonkey-compat",
  setup(b) {
    b.onLoad({ filter: /typebox[\\/].*\.mjs$/ }, async (args) => {
      if (/[\\/]format[\\/]idn_email\.mjs$/.test(args.path)) {
        return { contents: "export function IsIdnEmail(){return false;}", loader: "js" };
      }
      if (/[\\/]format[\\/]_idna\.mjs$/.test(args.path)) {
        return {
          contents:
            "export function IsIdnLabel(){return false;}\nexport function IsLabel(){return false;}",
          loader: "js",
        };
      }
      let contents = await readFile(args.path, "utf8");
      if (UNICODE_ID_REGEX_PATTERN.test(contents)) {
        contents = contents.replace(UNICODE_ID_REGEX_PATTERN, ASCII_ID_REGEX);
      }
      return { contents, loader: "js" };
    });
  },
};
