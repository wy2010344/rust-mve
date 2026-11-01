"""修正 `-> Fc` 的归属：只有 `#[rec]` 标注的函数才该有；并给回调字段标 required。"""

import io
import re
import sys

# 回调必填的组件：回调在 worker 里被直接调用，不能是 Option
REQUIRED = {
    'checkbox': 'on_toggle',
    'del_btn': 'on_click',
    'filter_btn': 'on_click',
    'primary_btn': 'on_click',
    'add_btn': 'on_click',
    'footer': 'on_add',
}


def process(path):
    src = io.open(path, encoding='utf-8').read()
    lines = src.split('\n')
    out = []
    n = 0
    for i, line in enumerate(lines):
        m = re.match(r'^fn (\w+)\(([^\n]*)\) -> Fc \{$', line)
        if m:
            prev = out[-1] if out else ''
            if not prev.startswith('#[rec'):
                out.append(f'fn {m.group(1)}({m.group(2)}) {{')
                n += 1
                continue
        out.append(line)
    text = '\n'.join(out)

    # 给回调字段声明必填
    for fn_name, field in REQUIRED.items():
        pat = re.compile(r'^#\[rec\]\n(fn ' + fn_name + r'\()', re.M)
        text, c = pat.subn('#[rec(required(' + field + '))]\\n\\1', text)
        n += c
        # 已是 `#[rec(..)]` 的情况保留原样
    io.open(path, 'w', encoding='utf-8').write(text)
    print(f'{path}: {n} 处修正')


if __name__ == '__main__':
    for p in sys.argv[1:]:
        process(p)
