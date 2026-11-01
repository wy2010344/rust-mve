"""修复「去 Some」过程中被吞掉的换行：`}   draw: |s| {` 应该断成两行。

判据：一行里 `}` 之后紧跟 ≥2 个空格再跟 `字段:`。
"""

import io
import re
import sys

FIELD = re.compile(r'\}\s{2,}([a-z_]+): ')


def reflow(text):
    out = []
    n = 0
    for line in text.split('\n'):
        m = FIELD.search(line)
        if m:
            indent = line[: len(line) - len(line.lstrip())]
            head = line[: m.start() + 1] + ','
            tail = m.group(1) + ': ' + line[m.end():]
            out.append(head)
            out.append(indent + '    ' + tail)
            n += 1
            continue
        out.append(line)
    return '\n'.join(out), n


if __name__ == '__main__':
    for path in sys.argv[1:]:
        src = io.open(path, encoding='utf-8').read()
        out, n = reflow(src)
        io.open(path, 'w', encoding='utf-8').write(out)
        print(f'{path}: {n} 处重排')
