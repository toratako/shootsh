# [SHOOT.SH](https://shoot.sh)

<h1 align="center"><code>ssh shoot.sh</code></h1>
<p align="center">
  <b>A terminal-based aim trainer.</b><br>
  Built with Rust, delivered via SSH.
</p>

## Run locally

```shell
git clone https://github.com/toratako/shootsh.git
cd shootsh
cargo run -p shootsh_cli --release
```

## Self-Hosting

See [deploy/](./deploy) (sample).  

## License

This project is licensed under [The Unlicense](LICENSE).  

### Third-Party Licenses

This project depends on several open-source libraries.  
For a complete list of their licenses, please see [THIRD-PARTY-LICENSES.html](THIRD-PARTY-LICENSES.html).  

```shell
# To (re)generate the third-party license list:
cargo about generate about.hbs > THIRD-PARTY-LICENSES.html
```
