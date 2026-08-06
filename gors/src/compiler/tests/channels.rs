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
