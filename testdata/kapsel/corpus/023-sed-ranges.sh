seq 1 10 > n.txt
sed -n '3,5p' n.txt
sed -n '2p' n.txt
sed -n '8,$p' n.txt
sed -n '/^7/p' n.txt
