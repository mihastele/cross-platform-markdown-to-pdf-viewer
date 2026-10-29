# mdpdf Showcase

A **dependency-free** Markdown to PDF engine written in *Rust*. It supports ~~nothing fancy~~ quite a lot: `inline code`, [links](https://www.rust-lang.org), and bare URLs like https://example.com/path.

## Text styles

This paragraph mixes **bold**, *italic*, ***bold italic***, and `code` spans, with a hard break  
right here, and a very long unbreakable token: Supercalifragilisticexpialidocious_Supercalifragilisticexpialidocious_Supercalifragilisticexpialidocious_end.

Unicode: café, naïve, “curly quotes”, em—dash, ellipsis…, arrows → and ← and ≤ ≥, euro €, emoji 🎉 (falls back to ?).

### Lists

- First item
- Second item with a long line that needs to wrap around to the next line so we can check hanging indents work as expected
  - Nested item
  - Another nested
    - Third level
- [x] Done task
- [ ] Open task

1. One
2. Two
3. Three
10. Ten (numbering follows start)

Paragraph between lists.

1. Loose item

   With a second paragraph.

2. Another loose item

## Quote

> Quotes can hold **formatting**.
>
> > And nest.
>
> - Even lists

## Code

```rust
fn main() {
    println!("Hello, PDF!");
    let very_long_line = "this line is intentionally very long so that it must be wrapped by the code block renderer instead of overflowing the page";
}
```

    indented code block

## Table

| Name | Qty | Price |
|:-----|:---:|------:|
| Apple | 3 | $1.20 |
| Banana with a much longer description that wraps | 12 | $0.50 |
| `code` cell | 7 | **$9.99** |

---

Reference link: [the Rust site][rust] and ![an image](pic.png).

[rust]: https://www.rust-lang.org

Setext Heading
==============

Setext H2
---------

Final paragraph. 

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two

## Filler section

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.

- bullet one
- bullet two
