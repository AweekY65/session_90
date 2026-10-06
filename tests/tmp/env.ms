export MS_A=1
echo $MS_A
unset MS_A
echo "[$MS_A]"
export MS_B=seen
env
