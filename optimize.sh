set -e
tools/aigtodot target/graph.aig target/graph1.dot
tools/abc -F cmd.txt
tools/aigtodot target/graph2.aig target/graph2.dot
