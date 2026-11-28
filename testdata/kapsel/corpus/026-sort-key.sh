printf 'x 3\ny 10\nz 2\n' > k.txt
sort -k 2 k.txt
sort -k 2 -n k.txt
sort -k 2,2 -n -r k.txt
printf 'a:3\nb:1\nc:2\n' | sort -t : -k 2 -n
