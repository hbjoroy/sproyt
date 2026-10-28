import assert from "node:assert/strict";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { MarkdownContent } from "../src/ui/react/markdown-content";

test("dice outcome renders the saved values as local transparent sprites with text fallback",()=>{
  const html=renderToStaticMarkup(<MarkdownContent source={'🎲 4 + 2 = 6\n\n```dice\n4,2\n```'} />);
  assert.ok(html.includes("4 + 2 = 6"));
  assert.ok(html.includes('/assets/dice/v1/4-a.png'));
  assert.ok(html.includes('/assets/dice/v1/2-b.png'));
  assert.ok(html.includes('aria-label="Terningkast: 4 og 2"'));
  assert.ok(!html.includes('<pre'));
});
test("invalid dice fences cannot request arbitrary paths or create an unbounded image list",()=>{
  for(const source of ['9,2','../secret','6,'.repeat(30),'6\n<script>']) {
    const html=renderToStaticMarkup(<MarkdownContent source={'```dice\n'+source+'\n```'} />);
    assert.ok(!html.includes('<img'));
  }
});
