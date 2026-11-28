# base64 and sha256sum, as python's base64/hashlib were measured to be used.
printf 'kapsel shell\n' > msg.txt
base64 msg.txt > msg.b64
cat msg.b64
base64 -d msg.b64 > round.txt
sha256sum msg.txt round.txt
sha256sum msg.txt > sums.txt
sha256sum -c sums.txt
printf 'abc' | sha256sum
printf 'abc' | base64 -w 0
