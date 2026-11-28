echo one | tee out.txt
echo two | tee -a out.txt
cat out.txt
echo three | tee a.txt b.txt > /dev/null
cat a.txt b.txt
printf 'x\n' | tee -a a.txt
cat a.txt
echo four | tee out.txt > /dev/null
cat out.txt
