package main

func main() {
	世界値 := "unicode identifiers"
	text := "source ok 世界"
	runeValue := '界'
	if 世界値 != "unicode identifiers" || text != "source ok 世界" || runeValue != 30028 {
		panic("source representation changed")
	}
}
