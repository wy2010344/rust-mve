import io
import re
import sys

PATTERN = re.compile(r'^\s*layout:\s*$')


def fix_layout(text):
    lines = text.split('\n')
    out = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if PATTERN.match(line):
            indent = line[: len(line) - len(line.lstrip())]
            body = []
            j = i + 1
            depth = 0
            started = False
            while j < len(lines):
                cur = lines[j]
                body.append(cur)
                for ch in cur:
                    if ch in '({[':
                        depth += 1
                        started = True
                    elif ch in ')}]':
                        depth -= 1
                if started and depth <= 0:
                    break
                j += 1
            # 手写 `Node2Arg { .. ..Node2Arg::default() }` 里的 layout 仍是 Option
            tail = '\n'.join(lines[j + 1 : j + 12])
            if '..Node2Arg::default()' in tail:
                out.append(indent + 'layout: Some(')
                out.extend((indent + '    ' + b) if b.strip() else b for b in body)
                out.append(indent + '),')
            else:
                out.append(line)
                out.extend(body[:-1])
                out.append(body[-1] + ',')
            i = j + 1
            continue
        out.append(line)
        i += 1
    return '\n'.join(out)


for path in sys.argv[1:]:
    src = io.open(path, encoding='utf-8').read()
    fixed = fix_layout(src)
    hits = len([l for l in src.split('\n') if PATTERN.match(l)])
    if fixed != src:
        io.open(path, 'w', encoding='utf-8').write(fixed)
    print(f'{path}: {hits} 处 layout')
