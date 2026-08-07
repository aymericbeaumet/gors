package main

func main() {
	one, two, three := 1, 2, 3
	five, six := 5, 6
	eight, four := 8, 4

	if one+two<<three != 17 {
		panic("shift must bind tighter than addition: 1 + (2<<3)")
	}
	if two<<one*three != 12 {
		panic("shift and multiplication have equal precedence, left assoc: (2<<1)*3")
	}
	if one<<two+three != 7 {
		panic("addition must bind looser than shift: (1<<2)+3")
	}
	if five|three^six != 1 {
		panic("| and ^ have equal precedence, left assoc: (5|3)^6")
	}
	if eight&^four+two != 10 {
		panic("bit clear must bind tighter than addition: (8&^4)+2")
	}
	if five&^one|two != 6 {
		panic("bit clear must bind tighter than or: (5&^1)|2")
	}
	if six&three == two != true {
		panic("& must bind tighter than ==: ((6&3) == 2) == true")
	}
	if ^one*two != -4 {
		panic("unary complement must bind tighter than *: (^1)*2")
	}
	if -two*three != -6 {
		panic("unary minus must bind tighter than *: (-2)*3")
	}
	p := &four
	if *p+one != 5 {
		panic("pointer indirection must bind tighter than +: (*p)+1")
	}
	quotient := five / two * three
	if quotient != 6 {
		panic("same-precedence operators must associate left to right: (5/2)*3")
	}
}
