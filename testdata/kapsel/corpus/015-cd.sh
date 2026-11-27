mkdir -p sub
cd sub
pwd > ../where.txt
cd ..
grep -c sub where.txt
