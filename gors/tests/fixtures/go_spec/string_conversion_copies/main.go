package main



func main() {
	text := "abc"
	bytes := []byte(text)
	bytes[0] = 'X'
	if text != "abc" {
		panic("[]byte conversion aliased the string")
	}
	rebuilt := string(bytes)
	bytes[1] = 'Y'
	if rebuilt != "Xbc" {
		panic("string conversion aliased the byte slice")
	}
	tail := text[1:]
	runes := []rune(text)
	runes[0] = 'Z'
	if text != "abc" || tail != "bc" {
		panic("[]rune conversion aliased the string")
	}
	println(text, rebuilt, string(bytes), tail, string(runes))
}
