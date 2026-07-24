# Stele Manual Test Suite: Code Highlighting

This file is for **manual visual testing** in a real terminal. Scroll through slowly. Each section
carries an *italic note* right above anything subtle, telling you what correct rendering should
look like. If what you see doesn't match the note, that's a bug — write down the section name and
keep going.

**Verified against `crates/highlight/Cargo.toml`**: the highlighter enables exactly 20 lumis
grammars — `rust`, `python`, `javascript`, `typescript`, `go`, `c`, `cpp`, `java`, `csharp`,
`ruby`, `swift`, `kotlin`, `zig`, `bash`, `json`, `yaml`, `toml`, `html`, `css`, `sql` — matching
this crate's own 20-language golden SGR snapshot set (`crates/highlight/tests/golden_sgr.rs`).
Languages sometimes expected in a highlighter — **Markdown, Haskell, Lua, PHP** — are **not**
compiled in here and were confirmed (by probing `lumis::languages::Language::guess` directly)
to fall back to a plain, unhighlighted block, exactly like a genuinely unknown tag. Common aliases
(`js`, `ts`, `py`, `rb`, `rs`, `cs`, `c++`, `cxx`, `c#`, `sh`, `yml`) were also probed and do
resolve to their canonical grammar — but `shell` and `tsx` do **not** resolve (no such alias/
extension is registered) and fall back to plain, same as an unsupported language.

**How to test `NO_COLOR`**: run

```
NO_COLOR=1 stele testdocs/05-code-highlighting.md
```

*expect: every code block below still shows its structural styling (code-block background/border,
inline-code markers, heading weight, etc.) but with zero color — no SGR color codes at all, only
non-color attributes if any. Compare against a plain `stele testdocs/05-code-highlighting.md` run
to confirm color is genuinely gone, not just dim.*

---

## Rust

```rust
// A small inventory tracker.
use std::collections::HashMap;

/// Adds `qty` units of `name`, returning the new total.
fn restock(stock: &mut HashMap<String, u32>, name: &str, qty: u32) -> u32 {
    let entry = stock.entry(name.to_string()).or_insert(0);
    *entry += qty;
    *entry
}

fn main() {
    let mut stock: HashMap<String, u32> = HashMap::new();
    let total = restock(&mut stock, "bolts", 42);
    println!("bolts in stock: {total}");
}
```

## Python

```python
"""Compute running statistics for a stream of numbers."""
from dataclasses import dataclass

@dataclass
class Stats:
    count: int = 0
    total: float = 0.0

    def add(self, value: float) -> None:
        self.count += 1
        self.total += value

    @property
    def mean(self) -> float:
        return self.total / self.count if self.count else 0.0

for n in [3, 7, 12.5, -1]:
    pass  # placeholder loop body
```

## JavaScript

```javascript
// Debounce a callback by `delay` milliseconds.
function debounce(fn, delay = 250) {
  let timer = null;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), delay);
  };
}

const log = debounce((msg) => console.log(`[debug] ${msg}`), 100);
const items = [1, 2, 3].map((n) => n * 2);
export default debounce;
```

## TypeScript

```typescript
interface User {
  id: number;
  name: string;
  roles?: string[];
}

function greet(user: User): string {
  const roles = user.roles ?? ["guest"];
  return `Hello, ${user.name} (${roles.join(", ")})`;
}

const admin: User = { id: 1, name: "Ada", roles: ["admin", "owner"] };
console.log(greet(admin));
```

## Go

```go
package main

import (
	"fmt"
	"strings"
)

// Shout uppercases s and appends n exclamation marks.
func Shout(s string, n int) string {
	return strings.ToUpper(s) + strings.Repeat("!", n)
}

func main() {
	fmt.Println(Shout("hello", 3))
}
```

## C

```c
#include <stdio.h>
#include <string.h>

/* Reverse a NUL-terminated string in place. */
void reverse(char *s) {
    size_t len = strlen(s);
    for (size_t i = 0; i < len / 2; i++) {
        char tmp = s[i];
        s[i] = s[len - 1 - i];
        s[len - 1 - i] = tmp;
    }
}

int main(void) {
    char buf[] = "stele";
    reverse(buf);
    printf("%s\n", buf);
    return 0;
}
```

## C++

```cpp
#include <iostream>
#include <vector>
#include <numeric>

template <typename T>
T sum(const std::vector<T>& xs) {
    return std::accumulate(xs.begin(), xs.end(), T{0});
}

int main() {
    std::vector<int> nums = {1, 2, 3, 4, 5};
    std::cout << "sum: " << sum(nums) << std::endl;
    return 0;
}
```

## Java

```java
import java.util.List;
import java.util.stream.Collectors;

public class Inventory {
    private final List<String> items;

    public Inventory(List<String> items) {
        this.items = items;
    }

    public String describe() {
        return items.stream()
            .map(String::toUpperCase)
            .collect(Collectors.joining(", "));
    }
}
```

## C#

```csharp
using System;
using System.Linq;

namespace Stele.Demo
{
    public class Inventory
    {
        private readonly int[] _counts;

        public Inventory(int[] counts) => _counts = counts;

        public double Average() => _counts.Length == 0 ? 0.0 : _counts.Average();
    }
}
```

## Ruby

```ruby
# Simple retry helper with exponential backoff.
def with_retry(max_attempts: 3)
  attempt = 0
  begin
    attempt += 1
    yield
  rescue StandardError => e
    retry if attempt < max_attempts
    raise e
  end
end

with_retry(max_attempts: 5) { puts "attempting..." }
```

## Swift

```swift
import Foundation

struct Point {
    let x: Double
    let y: Double

    func distance(to other: Point) -> Double {
        let dx = x - other.x
        let dy = y - other.y
        return (dx * dx + dy * dy).squareRoot()
    }
}

let a = Point(x: 0, y: 0)
let b = Point(x: 3, y: 4)
print("distance: \(a.distance(to: b))")
```

## Kotlin

```kotlin
data class Point(val x: Double, val y: Double) {
    fun distanceTo(other: Point): Double {
        val dx = x - other.x
        val dy = y - other.y
        return Math.sqrt(dx * dx + dy * dy)
    }
}

fun main() {
    val a = Point(0.0, 0.0)
    val b = Point(3.0, 4.0)
    println("distance: ${a.distanceTo(b)}")
}
```

## Zig

```zig
const std = @import("std");

fn fib(n: u32) u64 {
    if (n < 2) return n;
    var a: u64 = 0;
    var b: u64 = 1;
    var i: u32 = 1;
    while (i < n) : (i += 1) {
        const next = a + b;
        a = b;
        b = next;
    }
    return b;
}

pub fn main() void {
    std.debug.print("fib(10) = {}\n", .{fib(10)});
}
```

## Bash

```bash
#!/usr/bin/env bash
set -euo pipefail

# Back up every *.log file older than 7 days.
find /var/log -name "*.log" -mtime +7 | while read -r file; do
    dest="/backup/$(basename "$file").gz"
    gzip -c "$file" > "$dest"
    echo "archived: $file -> $dest"
done

exit 0
```

## JSON

```json
{
  "name": "stele",
  "version": "0.1.0",
  "private": true,
  "dependencies": {
    "ghostty-shim": "^2.0.0"
  },
  "features": ["highlight", "clip", "no-color"],
  "enabled": true,
  "max_width": 500
}
```

## YAML

```yaml
# CI pipeline for stele
name: ci
on:
  push:
    branches: [main]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run tests
        run: cargo test --workspace
    env:
      RUST_BACKTRACE: "1"
```

## TOML

```toml
[package]
name = "highlight"
version = "0.1.0"
edition = "2021"

[dependencies]
lumis = { version = "0.12.0", default-features = false }

[features]
default = ["lang-rust", "lang-python"]
max_line_len = 500
```

## HTML

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Stele Demo</title>
  </head>
  <body>
    <!-- main content -->
    <h1 class="title">Hello, &amp; welcome!</h1>
    <p data-count="3">Rendered at <em>runtime</em>.</p>
  </body>
</html>
```

## CSS

```css
:root {
  --accent: #4f46e5;
}

.card {
  display: flex;
  padding: 1rem 2rem;
  border-radius: 8px;
  background: linear-gradient(90deg, var(--accent), #22d3ee);
}

.card:hover::after {
  content: "→";
  opacity: 0.8;
}
```

## SQL

```sql
-- Top five customers by total spend in the last quarter.
SELECT c.id, c.name, SUM(o.total) AS spend
FROM customers AS c
JOIN orders AS o ON o.customer_id = c.id
WHERE o.created_at >= '2026-04-01'
GROUP BY c.id, c.name
HAVING SUM(o.total) > 100.0
ORDER BY spend DESC
LIMIT 5;
```

---

## No Language (Plain Fence)

*expect: a plain code-block background/border with no syntax coloring at all — every character
the same style, since there is no info string for the highlighter to key off of.*

```
def not_highlighted():
    return "this fence has no info string at all"
```

## Unknown Language (`flimflam`)

*expect: identical rendering to the plain fence above — an unrecognized language tag degrades
gracefully to a plain block. Confirmed directly against the crate: `Language::guess(Some("flimflam"), "")`
returns `PlainText`, and `highlight_line` returns a single unstyled run. This must never panic or
crash stele, regardless of how bogus the tag is.*

```flimflam
this looks like code but "flimflam" is not a real language tag
so it should render exactly like an unhighlighted block
```

## Extra Info-String Attributes

CommonMark's info string is "language, then anything else" — only the *first whitespace-delimited
token* is the language tag (see `crates/layout/src/block.rs`'s `walk_block` for `BlockKind::CodeBlock`).

*expect: `js {highlight=1}` — the extra `{highlight=1}` is a second token separated by a space, so
only `js` is taken as the language tag; this block should highlight as real JavaScript.*

```js {highlight=1}
const x = { a: 1, b: 2 };
console.log(x.a + x.b);
```

*expect: `rust,ignore` — this is a **subtle trap**, not a hypothetical one: because there is no
space before the comma, `rust,ignore` is a single whitespace token, and lumis has no alias or
extension entry for the literal string `"rust,ignore"`. Verified directly: `Language::guess(Some("rust,ignore"), "")`
returns `PlainText`. So despite looking like ordinary Rust doctest convention, this block falls
back to a **plain, unhighlighted** block — not Rust — and that is correct, expected behavior, not
a bug.*

```rust,ignore
fn this_should_not_highlight_as_rust() -> bool {
    true
}
```

## Indented Code Blocks (4-space)

*expect: a CommonMark indented code block (four leading spaces, no fence) renders as a literal
block with no language available — same plain styling as the no-language fence above, since an
indented code block has no info string to carry a language tag at all.*

    function indented() {
        return "four spaces of leading indentation, no fence markers";
    }

## Clipping: Lines Wider Than Any Terminal

Code blocks **clip rather than wrap** (`literal_block` in `crates/layout/src/block.rs`, via
`inline::clip_runs`). A line wider than the content width is truncated at the last full grapheme
cluster that fits and gets a trailing `…` indicator appended — it never spills onto a second row.

*expect: every long line below is cut off mid-line with a single trailing `…`, and the block never
grows a wrapped continuation line, no matter how the terminal is resized.*

```python
# A single overlong comment, ~500 characters, to force clipping: lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit lorem ipsum dolor sit amet consectetur adipiscing elit
short_line = "this one should render fully, unaffected by the line above"
```

*expect: the same clipping behavior in JavaScript — a single long array literal, no wrap.*

```javascript
const wide = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80];
```

*expect: same clipping behavior in Go — a single overlong string constant.*

```go
const banner = "GO GO GO ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------"
```

*expect: same clipping behavior in SQL — a single-line query with a very long `IN (...)` list.*

```sql
SELECT * FROM widgets WHERE id IN (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70);
```

## Tabs, Mixed Indentation, and Trailing Whitespace

*expect: tabs inside a code block are expanded to four spaces (see `literal_block` in
`crates/layout/src/block.rs`: `src.replace('\t', "    ")` before width measurement), so this block's
indentation should look consistent even though the source mixes tabs and spaces. Trailing
whitespace on a line should not visibly break anything or shift the line — it's just invisible
padding before the line's clip/pad boundary.

```python
def f():
	if True:
		return 1  
    else:   
        return 2	
```

## Literal Escape-Sequence-Looking Text

*expect: the characters below are literal text — a backslash followed by the letter `e`, and the
literal bracket-digit-letter sequence `[31m` — not a real ANSI escape byte (`0x1B`). This block must
render as plain highlighted/plain text with no color change mid-line and must never actually
colorize or otherwise control the real terminal, since these are printable characters, not control
bytes.*

```
literal escape look-alike: \e[31m this text should NOT turn red \e[0m
another: the two characters backslash and e, then [1;32mtext[0m as plain characters only
```

## CJK and Emoji Width

*expect: double-width CJK characters and emoji (some of which are themselves wide, some of which
carry variation selectors/ZWJ sequences) should not throw off column alignment, clipping math, or
cause visual overlap with the block's border — this exercises the same width engine the clip logic
in `inline::clip_runs` depends on.*

```javascript
// 你好，世界！ — comments can contain CJK and emoji: 🚀🔥✨
const 挨拶 = "こんにちは"; // Japanese identifier + string
const flags = "🇯🇵🇰🇷🇨🇳"; // flag emoji are multi-codepoint sequences
console.log(`${挨拶} ${flags} 👨‍👩‍👧‍👦`); // family emoji is a ZWJ sequence
```

## Inline Code Spans

*expect: every span below stays a single-line inline code span with its own subtle background/
border — never a fenced block, never wrapped mid-span, and empty/space-only spans should still be
visibly present (not collapse to invisible).*

- Backticks inside a span, using the "wrap in one extra backtick" CommonMark rule: `` `nested` ``
  (the span's literal content is `` `nested` ``, backticks included).
- A span containing only a single space character: ` `
- Two backticks with nothing between them — CommonMark can't actually form a zero-length code span
  from a single contiguous delimiter run, so this is expected to render as two literal backtick
  characters, not a broken/unstyled span: ``
- A very long inline span: `this_is_a_deliberately_long_inline_code_identifier_used_to_check_that_a_single_inline_span_does_not_break_line_layout_or_get_clipped_the_way_a_fenced_block_would`
- Adjacent to punctuation with no space: (`start()`), `end()`., `middle()`;and `x`,`y`,`z` back to back.


## A Long Block (200+ Lines)

*expect: this block is long enough to require real scrolling. Highlighting, clipping, and
row-by-row painting should stay correct and performant all the way through — no slowdown,
no visual glitches, no dropped or duplicated lines as you scroll.*

```python
"""Two hundred-plus lines of numbered widget classes, to exercise scrolling."""
from dataclasses import dataclass, field
from typing import Optional

@dataclass
class Widget1:
    """Widget number 1."""
    id: int = 1
    name: str = "widget_1"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget2:
    """Widget number 2."""
    id: int = 2
    name: str = "widget_2"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget3:
    """Widget number 3."""
    id: int = 3
    name: str = "widget_3"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget4:
    """Widget number 4."""
    id: int = 4
    name: str = "widget_4"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget5:
    """Widget number 5."""
    id: int = 5
    name: str = "widget_5"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget6:
    """Widget number 6."""
    id: int = 6
    name: str = "widget_6"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget7:
    """Widget number 7."""
    id: int = 7
    name: str = "widget_7"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget8:
    """Widget number 8."""
    id: int = 8
    name: str = "widget_8"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget9:
    """Widget number 9."""
    id: int = 9
    name: str = "widget_9"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget10:
    """Widget number 10."""
    id: int = 10
    name: str = "widget_10"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget11:
    """Widget number 11."""
    id: int = 11
    name: str = "widget_11"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget12:
    """Widget number 12."""
    id: int = 12
    name: str = "widget_12"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget13:
    """Widget number 13."""
    id: int = 13
    name: str = "widget_13"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget14:
    """Widget number 14."""
    id: int = 14
    name: str = "widget_14"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget15:
    """Widget number 15."""
    id: int = 15
    name: str = "widget_15"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget16:
    """Widget number 16."""
    id: int = 16
    name: str = "widget_16"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget17:
    """Widget number 17."""
    id: int = 17
    name: str = "widget_17"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget18:
    """Widget number 18."""
    id: int = 18
    name: str = "widget_18"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget19:
    """Widget number 19."""
    id: int = 19
    name: str = "widget_19"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget20:
    """Widget number 20."""
    id: int = 20
    name: str = "widget_20"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget21:
    """Widget number 21."""
    id: int = 21
    name: str = "widget_21"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget22:
    """Widget number 22."""
    id: int = 22
    name: str = "widget_22"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget23:
    """Widget number 23."""
    id: int = 23
    name: str = "widget_23"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget24:
    """Widget number 24."""
    id: int = 24
    name: str = "widget_24"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget25:
    """Widget number 25."""
    id: int = 25
    name: str = "widget_25"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget26:
    """Widget number 26."""
    id: int = 26
    name: str = "widget_26"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget27:
    """Widget number 27."""
    id: int = 27
    name: str = "widget_27"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget28:
    """Widget number 28."""
    id: int = 28
    name: str = "widget_28"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget29:
    """Widget number 29."""
    id: int = 29
    name: str = "widget_29"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget30:
    """Widget number 30."""
    id: int = 30
    name: str = "widget_30"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget31:
    """Widget number 31."""
    id: int = 31
    name: str = "widget_31"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget32:
    """Widget number 32."""
    id: int = 32
    name: str = "widget_32"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget33:
    """Widget number 33."""
    id: int = 33
    name: str = "widget_33"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget34:
    """Widget number 34."""
    id: int = 34
    name: str = "widget_34"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget35:
    """Widget number 35."""
    id: int = 35
    name: str = "widget_35"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget36:
    """Widget number 36."""
    id: int = 36
    name: str = "widget_36"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget37:
    """Widget number 37."""
    id: int = 37
    name: str = "widget_37"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget38:
    """Widget number 38."""
    id: int = 38
    name: str = "widget_38"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget39:
    """Widget number 39."""
    id: int = 39
    name: str = "widget_39"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget40:
    """Widget number 40."""
    id: int = 40
    name: str = "widget_40"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget41:
    """Widget number 41."""
    id: int = 41
    name: str = "widget_41"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget42:
    """Widget number 42."""
    id: int = 42
    name: str = "widget_42"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget43:
    """Widget number 43."""
    id: int = 43
    name: str = "widget_43"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget44:
    """Widget number 44."""
    id: int = 44
    name: str = "widget_44"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget45:
    """Widget number 45."""
    id: int = 45
    name: str = "widget_45"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget46:
    """Widget number 46."""
    id: int = 46
    name: str = "widget_46"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget47:
    """Widget number 47."""
    id: int = 47
    name: str = "widget_47"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget48:
    """Widget number 48."""
    id: int = 48
    name: str = "widget_48"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget49:
    """Widget number 49."""
    id: int = 49
    name: str = "widget_49"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

@dataclass
class Widget50:
    """Widget number 50."""
    id: int = 50
    name: str = "widget_50"
    tags: list = field(default_factory=list)

    def describe(self) -> str:
        return f"Widget #{self.id}: {self.name} ({len(self.tags)} tags)"

def build_all() -> list:
    return [
        Widget1(),
        Widget2(),
        Widget3(),
        Widget4(),
        Widget5(),
        Widget6(),
        Widget7(),
        Widget8(),
        Widget9(),
        Widget10(),
        Widget11(),
        Widget12(),
        Widget13(),
        Widget14(),
        Widget15(),
        Widget16(),
        Widget17(),
        Widget18(),
        Widget19(),
        Widget20(),
        Widget21(),
        Widget22(),
        Widget23(),
        Widget24(),
        Widget25(),
        Widget26(),
        Widget27(),
        Widget28(),
        Widget29(),
        Widget30(),
        Widget31(),
        Widget32(),
        Widget33(),
        Widget34(),
        Widget35(),
        Widget36(),
        Widget37(),
        Widget38(),
        Widget39(),
        Widget40(),
        Widget41(),
        Widget42(),
        Widget43(),
        Widget44(),
        Widget45(),
        Widget46(),
        Widget47(),
        Widget48(),
        Widget49(),
        Widget50(),
    ]
```

## Nested Fences

CommonMark requires an outer fence's delimiter run to be *longer* than any backtick run appearing
in its content, so a fence containing a fence must open with four (or more) backticks.

*expect: the outer four-backtick fence is treated as one plain (unlabeled) code block, and the
inner three-backtick fence shown inside it renders as **literal text**, not as a real nested,
separately-highlighted code block — stele's fence parsing is not recursive.*

````
Here is how you'd show a fenced code example inside documentation:

```python
def hello():
    print("hello from inside a nested fence")
```

That inner fence should stay inert literal text, not actually highlighted Python.
````
