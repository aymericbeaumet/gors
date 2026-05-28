package main

import "fmt"

type Counter struct {
	Embedded
	Value int
}

func (c Counter) Label() string {
	return "counter"
}

func (c Counter) Detail() string {
	return c.Name
}

type Embedded struct {
	Name string
}

type TaggedEmbedded struct {
	Code int
}

type TaggedWrapper struct {
	*TaggedEmbedded
	Label string `json:"label"`
}

type ShadowEmbedded struct {
	Value int
}

type ShadowWrapper struct {
	*ShadowEmbedded
	Value int
}

type Accumulator struct {
	Total int
}

func (a *Accumulator) Add(value int) {
	a.Total += value
}

func (a *Accumulator) Sum() int {
	return a.Total
}

type Namer interface {
	Label() string
}

type Adder interface {
	Add(int)
	Sum() int
}

type Detailer interface {
	Detail() string
}

type Describer interface {
	Namer
	Detailer
}

type Node struct {
	Value    int
	Children []Node
}

func Name(n Namer) string {
	return n.Label()
}

func Describe(d Describer) string {
	return d.Label() + ":" + d.Detail()
}

func AddOne(a Adder) int {
	a.Add(1)
	return a.Sum()
}

func SendOnly(ch chan<- int, value int) {
	ch <- value
}

func ReceiveOnly(ch <-chan int) int {
	return <-ch
}

func main() {
	booleans := true && !false
	integer := 4 + 2
	float := 2.5 + 0.5
	complexValue := complex(1, 2)
	text := "gors"
	array := [2]int{1, 2}
	slice := []int{1, 2, 3}
	slice = append(slice, 4)
	structValue := Counter{Embedded: Embedded{Name: "score"}, Value: array[1]}
	var pointer *Counter = &structValue
	mapping := map[string]int{"answer": 42}
	channel := make(chan int, 1)
	channel <- mapping["answer"]
	sendDirectional := make(chan int, 1)
	SendOnly(sendDirectional, 7)
	receiveDirectional := make(chan int, 1)
	receiveDirectional <- 8
	node := Node{Value: 1, Children: []Node{{Value: 2}}}
	wrapper := TaggedWrapper{TaggedEmbedded: &TaggedEmbedded{Code: 9}, Label: "tagged"}
	shadow := ShadowWrapper{ShadowEmbedded: &ShadowEmbedded{Value: 1}, Value: 2}
	accumulator := Accumulator{Total: 4}
	var dynamicAdder any = &accumulator
	assertedAdder, assertedAdderOK := dynamicAdder.(Adder)
	assertedAdder.Add(2)
	typeSwitchTotal := 0
	switch typedAdder := dynamicAdder.(type) {
	case Adder:
		typedAdder.Add(3)
		typeSwitchTotal = typedAdder.Sum()
	default:
		typeSwitchTotal = -1
	}
	function := func(value int) int { return value + pointer.Value }
	var nilPointer *int
	var nilSlice []int
	var nilMap map[string]int
	var nilChan chan int
	var nilFunc func() int
	var nilInterface interface{}
	fmt.Println(booleans, integer, float, real(complexValue), imag(complexValue), text[0], len(slice), structValue.Name, Name(structValue), Describe(Counter{Embedded: Embedded{Name: "detail"}, Value: 1}), <-channel, ReceiveOnly(receiveDirectional), node.Children[0].Value, wrapper.Code, wrapper.TaggedEmbedded.Code, wrapper.Label, shadow.Value, shadow.ShadowEmbedded.Value, assertedAdderOK, assertedAdder.Sum(), typeSwitchTotal, AddOne(&accumulator), accumulator.Total, function(3), nilPointer == nil, nilSlice == nil, nilMap == nil, nilChan == nil, nilFunc == nil, nilInterface == nil)
}
