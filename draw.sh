set -e


dot graph0.dot -Tsvg -o graph0.svg

tools/aigtodot ok.aig > ok.dot
dot ok.dot -Tsvg -o ok.svg