"""把 `Some(Flex { direction: always(..), ..Default::default() })` 转成 `flex! { .. }`。

counter.rs 走的是手写结构体那套（`Flex` 字段本身是 getter），新形态下统一由
`#[rec] fn flex` 生成的 `flex!` 宏产出具体值。
"""

import io
import re
import sys


def convert(text):
    out, i, n = [], 0, 0
    while True:
        m = re.compile(r'Some\(Flex\s*\{').search(text, i)
        if not m:
            out.append(text[i:])
            break
        open_at = text.index('{', m.start())
        depth, k = 1, open_at + 1
        while k < len(text) and depth:
            if text[k] == '{':
                depth += 1
            elif text[k] == '}':
                depth -= 1
            k += 1
        inner = text[open_at + 1:k - 1]
        # Some( 与 Flex{ 之间可能有缩进/换行
        indent = ' ' * (m.start() - (text.rfind('\n', 0, m.start()) + 1))
        fields = []
        for line in inner.split('\n'):
            s = line.strip()
            if not s or s.startswith('..Default::default()'):
                continue
            name, _, val = s.partition(':')
            val = val.strip().rstrip(',')
            mm = re.fullmatch(r'always\((.*)\)', val)
            if mm:
                val = mm.group(1)
            fields.append(f'{indent}    {name.strip()}: {val},')
        rendered = 'flex! {\n' + '\n'.join(fields) + '\n' + indent + '}'
        out.append(text[i:m.start()])
        out.append(rendered)
        # 吃掉原 `Some(Flex { .. })` 尾部的 `)`
        rest = k
        if rest < len(text) and text[rest] == ')':
            rest += 1
        i = rest
        n += 1
    return ''.join(out), n


if __name__ == '__main__':
    for path in sys.argv[1:]:
        src = io.open(path, encoding='utf-8').read()
        out, n = convert(src)
        io.open(path, 'w', encoding='utf-8').write(out)
        print(f'{path}: {n} 处 flex! 转换')
