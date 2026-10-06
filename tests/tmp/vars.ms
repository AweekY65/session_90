export MS_FOO=bar
echo $MS_FOO
echo ${MS_FOO}
echo "x${MS_FOO}y"
echo "[$MS_NO_SUCH_VAR]"
echo $?
false
echo $?
