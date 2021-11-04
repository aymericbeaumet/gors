

package


main


import   "lib/math" 
import m "lib/math"
import . "lib/math"
import _ "lib/math"

import (
    "lib/math" 
  m "lib/math"
  . "lib/math"
  _ "lib/math"
)

import   `lib/math` 
import m `lib/math`
import . `lib/math`
import _ `lib/math`

import (
    `lib/math` 
  m `lib/math`
  . `lib/math`
  _ `lib/math`
)


func


main(


) {

10
0b10
0B10
0o10
0O10
0x10
0X10

10_
0b1_0_
0B1_0_
0o1_0_
0O1_0_
0x1_0_
0X1_0_

a

'a'
'ä'
'本'
'\t'
'\000'
'\007'
'\377'
'\x07'
'\xff'
'\u12e4'
'\U00101234'
'\''         // rune literal containing single quote character

`abc`                // same as "abc"
`\n
\n`                  // same as "\\n\n\\n"
"\n"
"\""                 // same as `"`
"Hello, world!\n"
"日本語"
"\u65e5本\U00008a9e"
"\xff\u00FF"

a + a
a - a
a * a
a / a
a % a

a & a
a | a
a ^ a
a << a
a >> a
a &^ a

a += a
a -= a
a *= a
a /= a
a %= a

a &= a
a -= a
a ^= a
a <<= a
a >>= a
a &^= a

a && a
a || a
a <- a
a++
a--

a == a
a < a
a > a
a = a
!a

a != a
a <= a
a >= a
a := a
func (a ...int) { fmt.Println(a...) }

a[a]
{}
func(a int, b int) {}

go func() {
	defer func() {}()

	const a = "Hello World"
	type anon struct{}
	var b = make(chan anon)
	c := map[string]interface{}{}

	switch a {
		case "a":
			break
		case "b":
			fallthrough
		case "c":
			goto End
		case "d":
			return
		default:
			panic("default")
	}
	End:

	for range b {
		continue
	}

	if true { } else { }

	select {}
}()

};


