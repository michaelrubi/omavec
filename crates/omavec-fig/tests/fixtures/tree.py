# Ground truth for Omavec's .fig spike: the node tree of a .fig file as
# fig2sketch's pure-Python kiwi decoder reads it, one node per line.
import sys, zipfile
sys.path.insert(0, "src")
from figformat import kiwi

def tree(path):
    reader = open(path, "rb")
    if reader.read(2) == b"PK":
        reader = zipfile.ZipFile(path).open("canvas.fig")
    else:
        reader.seek(0)
    fig = kiwi.decode(reader, {"GUID": lambda x: (x["sessionID"], x["localID"])})
    nodes, root = {}, None
    for node in fig["nodeChanges"]:
        node["children"] = []
        nodes[node["guid"]] = node
        root = root or node["guid"]
    for node in nodes.values():
        if "parentIndex" in node:
            nodes[node["parentIndex"]["guid"]]["children"].append(node)
    lines = []
    def walk(node, depth):
        node["children"].sort(key=lambda n: n["parentIndex"]["position"])
        lines.append("%s%s %d:%d %s" % ("  " * depth, node["type"], node["guid"][0], node["guid"][1], node.get("name", "")))
        for child in node["children"]:
            walk(child, depth + 1)
    walk(nodes[root], 0)
    return lines

for path in sys.argv[1:]:
    out = path.rsplit(".", 1)[0] + ".tree.txt"
    lines = tree(path)
    open(out, "w").write("\n".join(lines) + "\n")
    print(path, len(lines), "nodes")
