use super::compile_and_run;

#[test]
fn generated_integer_channels_preserve_direction_close_and_comma_ok_semantics() {
    let run = compile_and_run(
        r#"
            package main

            func send(channel chan<- int, value int) { channel <- value }
            func receive(channel <-chan int) int { return <-channel }
            func closedSendPanics(channel chan int) (panicked bool) {
                defer func() { panicked = recover() != nil }()
                channel <- 99
                return false
            }

            func main() {
                var nilChannel chan int
                if nilChannel != nil || len(nilChannel) != 0 || cap(nilChannel) != 0 {
                    panic("invalid nil channel")
                }

                channel := make(chan int, 2)
                send(channel, 11)
                send(channel, 22)
                if len(channel) != 2 || cap(channel) != 2 {
                    panic("invalid channel size")
                }
                close(channel)

                first, firstOK := <-channel
                second := receive(channel)
                last, lastOK := <-channel
                if first != 11 || !firstOK || second != 22 || last != 0 || lastOK {
                    panic("invalid receive sequence")
                }
                if !closedSendPanics(channel) {
                    panic("send after close did not panic")
                }
                println("channels: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"channels: ok\n");
    assert!(run.rust.contains("GoChannelI64"), "{}", run.rust);
    assert!(run.rust.contains("go_channel_i64_receive"), "{}", run.rust);
    assert!(run.rust.contains("let (__gors_result_"), "{}", run.rust);
}

#[test]
fn generated_string_channels_preserve_fifo_select_close_and_zero_values() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                channel := make(chan string, 3)
                select {
                case channel <- "a":
                default:
                    panic("buffered string send was not ready")
                }
                channel <- "b"
                channel <- "c"
                if len(channel) != 3 || cap(channel) != 3 {
                    panic("invalid string channel size")
                }

                var first string
                var firstOK bool
                select {
                case first, firstOK = <-channel:
                default:
                    panic("buffered string receive was not ready")
                }
                channel <- "d"
                close(channel)
                second, secondOK := <-channel
                third, thirdOK := <-channel
                fourth, fourthOK := <-channel
                zero, zeroOK := <-channel
                if first != "a" || !firstOK || second != "b" || !secondOK ||
                    third != "c" || !thirdOK || fourth != "d" || !fourthOK ||
                    zero != "" || zeroOK || len(channel) != 0 {
                    panic("invalid string channel receive sequence")
                }
                println("string-channels: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"string-channels: ok\n");
    assert!(run.rust.contains("GoChannelGoString"), "{}", run.rust);
    assert!(
        run.rust.contains("go_channel_go_string_try_send"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_channel_go_string_try_receive"),
        "{}",
        run.rust
    );
}

#[test]
fn generated_directional_and_nested_channels_share_one_runtime_representation() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                channel := make(chan int, 2)
                send := (chan<- int)(channel)
                receive := (<-chan int)(channel)
                var assigned chan<- int = channel
                send <- 1
                assigned <- 2
                if len(send) != 2 || cap(receive) != 2 || <-receive != 1 || <-receive != 2 {
                    panic("directional views did not share storage")
                }
                close(send)
                value, open := <-receive
                if value != 0 || open {
                    panic("directional close changed")
                }

                inner := make(chan int, 1)
                inner <- 42
                nested := make(chan (<-chan int), 1)
                var nestedSend chan<- <-chan int = nested
                nestedSend <- inner
                if <-<-nested != 42 {
                    panic("nested receive-only channel changed")
                }

                inner2 := make(chan int, 1)
                inner2 <- 43
                nested2 := make(chan chan int, 1)
                var nestedSend2 chan<- chan int = nested2
                nestedSend2 <- inner2
                if <-<-nested2 != 43 {
                    panic("nested bidirectional channel changed")
                }

                sendOnly := make(chan<- int, 1)
                sendOnly <- 7
                if len(sendOnly) != 1 || cap(sendOnly) != 1 {
                    panic("direct directional make changed")
                }
                println("directional-nested-channels: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"directional-nested-channels: ok\n");
    assert!(run.rust.contains("GoChannelGoChannelI64"), "{}", run.rust);
    assert!(
        run.rust.contains("go_channel_go_channel_i64_send"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_channel_go_channel_i64_receive_value"),
        "{}",
        run.rust
    );
}
