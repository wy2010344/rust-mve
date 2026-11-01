"""node2 demo: `#[tfc]` -> `#[rec]` 的完整机械迁移。

从 git staged 基线出发一次性走完：
  1. `#[tfc]` / `#[tfc(..)]`  ->  `#[rec]` / `#[rec(..)]`
  2. 删掉 `cx: &mut NodeContext` 形参与 `let _ = cx;`（rec 不注入 ctx）
  3. 返回类型补 `-> Fc`
  4. `!` 宏调用后又跟的 `.of()` 删掉（宏产物已是 `Fc`；手写函数的保留）
  5. **宏块内**的 `layout: Some(flex!{..})` -> `layout: flex!{..}`
     （手写 `Node2Arg { .. }` 字面量不动，那里的字段本来就是 `Option<..>`）
  6. 字段简写：`store: store,` -> `store,`
"""

import io
import re
import sys

DELIM_OPEN = '([{'
DELIM_CLOSE = ')]}'


def find_macro_blocks(text):
    """找出所有 `xxx! { .. }` 宏调用块的 (起点, 花括号起点, 终点之后)。"""
    blocks = []
    for m in re.finditer(r'[A-Za-z_]\w*!\s*[({]', text):
        open_brace = text.index('{', m.start()) if '{' in text[m.start():m.end()] else text.index('(', m.start())
        depth = 0
        k = open_brace
        started = False
        while k < len(text):
            ch = text[k]
            if ch in DELIM_OPEN:
                depth += 1
                started = True
            elif ch in DELIM_CLOSE:
                depth -= 1
            if started and depth <= 0:
                break
            k += 1
        blocks.append((m.start(), open_brace, k))
    return blocks


def strip_some_in_blocks(text):
    """宏块内的 `Some(<inner>)` -> `<inner>`，返回 (新文本, 处数)。"""
    blocks = find_macro_blocks(text)
    edits = []
    for i, j, k in blocks:
        seg = text[i:k]
        for m in re.finditer(r'Some\(\s*flex!', seg):
            start = i + m.start()
            inner_at = start + text[start:].index('(') + 1
            depth, e = 1, inner_at
            while e < len(text) and depth:
                if text[e] == '(':
                    depth += 1
                elif text[e] == ')':
                    depth -= 1
                e += 1
            edits.append((start, e, text[inner_at:e - 1]))
    for start, end, new in reversed(edits):
        text = text[:start] + new + text[end:]
    return text, len(edits)


def drop_macro_of(text):
    """删掉宏调用后紧跟的 `.of()`（`xxx!{..}.of()`），手写函数的不动。"""
    blocks = find_macro_blocks(text)
    out, prev = [], 0
    n = 0
    # 必须按结束位置推进，并跳过已被外层块覆盖的内层块，否则 `prev` 会倒退、
    # 造成整段重复输出（嵌套的 `abs_box!{..}.of()` 就踩过这个坑）。
    for i, j, k in sorted(blocks, key=lambda b: b[2]):
        if k < prev:
            continue
        rest = text[k + 1:]
        m = re.match(r'\s*\.of\(\)', rest)
        if not m:
            continue
        out.append(text[prev:k + 1])
        prev = k + 1 + m.end()
        n += 1
    out.append(text[prev:])
    return ''.join(out), n


def migrate(text):
    n = 0
    for old, new in (('#[tfc(', '#[rec('), ('#[tfc]', '#[rec]')):
        c = text.count(old)
        if c:
            text = text.replace(old, new)
            n += c
    for pat in (r'^    cx: &mut NodeContext,\n', r'^    let _ = cx;\n'):
        text, c = re.subn(pat, '', text, flags=re.M)
        n += c
    text, c = re.subn(r'cx: &mut NodeContext, ', '', text)
    n += c
    text, c = re.subn(r'^\) \{$', ') -> Fc {', text, flags=re.M)
    n += c
    text, c = re.subn(r'^fn (\w+\([^\n]*\)) \{$', r'fn \1 -> Fc {', text, flags=re.M)
    n += c
    text, c = drop_macro_of(text)
    n += c
    text, c = strip_some_in_blocks(text)
    n += c
    # 去掉残留的重复逗号：保留第一个，保留缩进，不去换行
    text = re.sub(r',(\s*\n)(\s*),', lambda m: ',\n' + m.group(2), text)
    text = re.sub(r',[ \t]*,', ',', text)
    text, c = re.subn(r'\b([A-Za-z_]\w*): \1(?=,| ?\}|$)', r'\1', text)
    n += c
    text = re.sub(r'\buse wy_mve::\{[^}]*\}', lambda m: m.group(0).replace('tfc', 'rec'), text)
    return text, n


if __name__ == '__main__':
    for path in sys.argv[1:]:
        src = io.open(path, encoding='utf-8').read()
        out, n = migrate(src)
        io.open(path, 'w', encoding='utf-8').write(out)
        print(f'{path}: {n} 处改动')
