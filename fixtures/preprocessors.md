# Source-text preprocessors

Constructs the `decor::d2l`, `decor::quarto` and `decor::codecogs` passes
rewrite before the parse. Every line here is modelled on a real document —
d2l's book source, Quarto's markdown-basics page, and a README that predates
GitHub's `$…$` support.

## d2l roles
:label:`sec_preprocessors`

An own-line label names a target the reader never sees.

![The transformer architecture](../img/transformer.svg)
:width:`320px`
:label:`fig_transformer`

$$\hat{y} = \mathbf{w}^\top \mathbf{x} + b$$
:eqlabel:`eq_price-area`

:numref:`fig_transformer` depicts the model, and :eqref:`eq_price-area`
prices it. Least squares :cite:`Legendre.1805,Gauss.1809` is old, and
:citet:`Hubel.Wiesel.1959` explored the visual cortex.

![Figure taken from :citet:`Field.1987`: coding with six channels.](../img/field-visual.png)

:begin_tab:`mxnet`

The MXNet build reads the data lazily.

:end_tab:

:begin_tab:`pytorch, tensorflow`

Both of these read it eagerly.

:end_tab:

```{.python .input}
%%tab all
def load(batch_size):
    return batch_size * 2
```

```{.python .input}
%%tab pytorch, mxnet, tensorflow
x = {1: 2}
```

## Quarto callouts

::: {.callout-note}
## No blank lines in display math

The delimiters may be separated from the formula by whitespace.

However, there can be no blank lines between them.
:::

:::{.callout-tip}
A tip written with no space after the colons.
:::

::: callout-important
A callout written with a bare class and no braces.
:::

:::warning
A Docusaurus-style admonition, which is not in the corpus but is the
commonest one in the wild.
:::

::::: {.callout-caution}
An outer callout, five colons deep.

::: {.callout-note}
An inner callout, three colons deep, closed by three.
:::

Still inside the outer one.
:::::

::: {.callout-note}
A callout whose body holds a fence:

```python
x = 1

y = 2
```
:::

## Spans and shortcodes {#spans-and-shortcodes}

[This text is smallcaps]{.smallcaps}, [this is underlined]{.underline}, and
[this is *some text*]{.class key="val"}.

{{< include _pagebreak.qmd >}}

## CodeCogs equations

Percent-encoded: ![equals1](http://latex.codecogs.com/svg.latex?x%20%3A%3D%202kj)

Pseudo-entities: ![complex](http://latex.codecogs.com/svg.latex?a&space;&plus;&space;ib)

Both at once: ![sum2](http://latex.codecogs.com/svg.latex?%5Csum_%7Bi%3D1%7D%5E%7B100%7D%282i&plus;1%29)

An `https` PNG endpoint with a render directive:
![dpi](https://latex.codecogs.com/png.latex?%5Cdpi%7B110%7D%5Cinline%20x%5E2)
