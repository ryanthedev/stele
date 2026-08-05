# Remote images

Every image below has a destination stele will not open on its own. With no
flag it makes no request at all and each one renders as its alt text, which is
what the goldens beside this file record.

A plain HTTPS image:

![a scatter plot of the residuals](https://example.com/diagrams/residuals.png)

An SVG, which `gfx` renders as well as it renders a raster — the format is
sniffed from the bytes, so a fetched vector goes down exactly the same path:

![the network architecture](https://example.com/diagrams/architecture.svg)

Plain HTTP, and one carrying a title:

![an older diagram](http://example.com/legacy/plot.png)

![the loss curve](https://example.com/diagrams/loss.png "Training loss, 40 epochs")

Two on one line, with prose between them:
![the first pass](https://example.com/a.png) then ![the second](https://example.com/b.png).

A URL with parentheses in it, which a scanner that stops at the first `)` gets
wrong:

![a file with brackets](https://example.com/plot_(final).png)

## Things that must never be fetched

A **link** to an image is not an image. Following it is the reader's decision,
made by pressing a key, not a consequence of opening the page:

[the full-size version](https://example.com/diagrams/residuals.png)

An autolink is not one either: <https://example.com/diagrams/residuals.png>

Schemes other than `http` and `https` are refused by name, on the original URL
and on every redirect hop:

![a local file read](file:///etc/passwd)

![bytes smuggled inline](data:image/png;base64,iVBORw0KGgo=)

Anything inside a fence is quoted, not requested — a document *about* remote
images would otherwise fetch every URL it documents:

```markdown
![this is an example, not a request](https://example.com/never-fetched.png)
```

    ![indented code is code too](https://example.com/also-never-fetched.png)

## Local images are unaffected

The pipeline below this pass does not know a network exists, so a relative path
behaves exactly as it always has:

![a local diagram](./img/cat.png)
