printf 'a:b:c\nd:e:f\n' > f.txt
cut -d : -f 2 f.txt
cut -d : -f 1,3 f.txt
cut -d : -f 2- f.txt
printf 'abcdef\n' | cut -c 2-4
