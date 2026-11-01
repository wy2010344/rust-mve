"""把 `flex().a().b()` 这类 pipeline 改写成 `flex! { a: .., b: .. }`。

形态一（单行）：`flex().align_item(AlignItem::Stretch).align_fix(true)`
形态二（多行）：
    flex()
        .direction(Layout::Column)
        .gap(10.0),

输出统一为 `flex! { <多行时换行对齐> }`，并保留调用点原有的尾随逗号位置。
"""

import io
import re
import sys


def collect_chain(text, start):
    """从 `flex()` 的位置 start 开始，吞掉后续 `.m(args)` 链。

    返回 (消耗到的下标, [(方法名, 参数文本), ...])。
    """
    i = start + len('flex()')
    calls = []
    while True:
        # 允许跨行空白
        j = i
        while j < len(text) and text[j] in ' \t\r\n':
            j += 1
        if j >= len(text) or text[j] != '.':
            break
        k = j + 1
        while k < len(text) and (text[k].isalnum() or text[k] == '_'):
            k += 1
        method = text[j + 1:k]
        if k >= len(text) or text[k] != '(':
            break
        depth, e = 1, k + 1
        while e < len(text) and depth:
            if text[e] == '(':
                depth += 1
            elif text[e] == ')':
                depth -= 1
            e += 1
        calls.append((method, text[k + 1:e - 1]))
        i = e
    return i, calls


def macroize(text):
    out = []
    i = 0
    n = 0
    while True:
        m = re.search(r'\bflex\(\)', text[i:])
        if not m:
            out.append(text[i:])
            break
        start = i + m.start()
        indent_start = text.rfind('\n', 0, start) + 1
        base_indent = text[indent_start:start].expandtabs()
        end, calls = collect_chain(text, start)
        multi_line = '\n' in text[start:end]
        if multi_line:
            body = '\n'.join(
                f'{base_indent}    {name}: {arg},' for name, arg in calls
            )
            rendered = 'flex! {\n' + body + '\n' + base_indent + '}'
        else:
            rendered = 'flex! { ' + ', '.join(f'{name}: {arg}' for name, arg in calls) + ' }'
        out.append(text[i:start])
        out.append(rendered)
        i = end
        n += 1
    return ''.join(out), n


if __name__ == '__main__':
    for path in sys.argv[1:]:
        src = io.open(path, encoding='utf-8').read()
        out, n = macroize(src)
        io.open(path, 'w', encoding='utf-8').write(out)
        print(f'{path}: {n} 处 flex pipeline')
