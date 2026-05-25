package main

import (
	"fmt"
	"strings"
)

func isVowel(r rune) bool {
	return r == 'a' || r == 'e' || r == 'i' || r == 'o' || r == 'u'
}

func dashRune(r rune) rune {
	if isVowel(r) {
		return '-'
	}
	return r
}

func main() {
	fmt.Println(strings.Clone("clone"))
	fmt.Println(strings.Compare("a", "b"))
	fmt.Println(strings.Contains("alpha", "ph"))
	fmt.Println(strings.ContainsAny("alpha", "xyzp"))
	fmt.Println(strings.ContainsFunc("alpha", isVowel))
	fmt.Println(strings.ContainsRune("alpha", 'p'))
	fmt.Println(strings.Count("banana", "na"))

	before, after, cut := strings.Cut("key=value", "=")
	fmt.Println(before, after, cut)
	withoutPrefix, hadPrefix := strings.CutPrefix("prefix-value", "prefix-")
	fmt.Println(withoutPrefix, hadPrefix)
	withoutSuffix, hadSuffix := strings.CutSuffix("value-suffix", "-suffix")
	fmt.Println(withoutSuffix, hadSuffix)

	fmt.Println(strings.EqualFold("Go", "go"))
	fmt.Println(strings.Fields(" a  b c "))
	fmt.Println(strings.FieldsFunc("a,b;c", func(r rune) bool { return r == ',' || r == ';' }))
	fmt.Println(strings.HasPrefix("gors", "go"))
	fmt.Println(strings.HasSuffix("gors", "rs"))
	fmt.Println(strings.Index("gopher", "ph"))
	fmt.Println(strings.IndexAny("gopher", "xyzp"))
	fmt.Println(strings.IndexByte("gopher", 'p'))
	fmt.Println(strings.IndexFunc("gopher", isVowel))
	fmt.Println(strings.IndexRune("gopher", 'h'))
	fmt.Println(strings.Join([]string{"a", "b", "c"}, "|"))
	fmt.Println(strings.LastIndex("banana", "na"))
	fmt.Println(strings.LastIndexAny("banana", "ab"))
	fmt.Println(strings.LastIndexByte("banana", 'a'))
	fmt.Println(strings.LastIndexFunc("banana", isVowel))
	fmt.Println(strings.Map(dashRune, "banana"))
	fmt.Println(strings.Repeat("ab", 3))
	fmt.Println(strings.Replace("banana", "na", "NA", 1))
	fmt.Println(strings.ReplaceAll("banana", "na", "NA"))
	fmt.Println(strings.Split("a,b,c", ","))
	fmt.Println(strings.SplitAfter("a,b,c", ","))
	fmt.Println(strings.SplitAfterN("a,b,c", ",", 2))
	fmt.Println(strings.SplitN("a,b,c", ",", 2))
	fmt.Println(strings.Title("hello gors"))
	fmt.Println(strings.ToLower("HELLO"))
	fmt.Println(strings.ToTitle("hello"))
	fmt.Println(strings.ToUpper("hello"))
	fmt.Println(strings.ToValidUTF8("hello", "?"))
	fmt.Println(strings.Trim("abba", "ab"))
	fmt.Println(strings.TrimFunc("banana", isVowel))
	fmt.Println(strings.TrimLeft("abba", "ab"))
	fmt.Println(strings.TrimLeftFunc("banana", isVowel))
	fmt.Println(strings.TrimPrefix("prefix-value", "prefix-"))
	fmt.Println(strings.TrimRight("abba", "ab"))
	fmt.Println(strings.TrimRightFunc("banana", isVowel))
	fmt.Println(strings.TrimSpace("  hello  "))
	fmt.Println(strings.TrimSuffix("value-suffix", "-suffix"))
}
