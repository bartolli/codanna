# highlight.js bundles

Built from npm `highlight.js@11.12.0` (BSD-3-Clause, `LICENSE-highlightjs`).
`core.min.js` exposes `window.hljs`; each `<lang>.min.js` registers one grammar
onto it. `graph.mjs` inlines core plus only the grammars for languages present
in the dump, and refuses any bundle in which `findNetworkPrimitives()` finds a
network call.

## Recipe (esbuild 0.25)

```
npm pack highlight.js@11 && tar xzf highlight.js-*.tgz
# core-entry.js:
#   import hljs from "./package/es/core.js";
#   window.hljs = hljs;
# lang-<l>.js, one per language:
#   import l from "./package/es/languages/<l>.js";
#   window.hljs.registerLanguage("<l>", l);
esbuild core-entry.js --bundle --minify --format=iife --outfile=core.min.js
esbuild lang-<l>.js  --bundle --minify --format=iife --outfile=<l>.min.js
```

## Languages

The codanna parsers with a core hljs grammar: c, clojure, cpp, csharp, go,
java, javascript, kotlin, lua, php, python, rust, swift, typescript.
gdscript has no core grammar; its signatures render as plain text.
`typescript.min.js` bundles the javascript grammar it imports, but registers
only `typescript`.
