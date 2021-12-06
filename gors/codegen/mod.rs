pub fn fprint<W: std::io::Write>(
    mut w: W,
    file: syn::File,
) -> Result<(), Box<dyn std::error::Error>> {
    let out = quote::quote! {#file};
    w.write_all(out.to_string().as_bytes())?;
    Ok(())
}
