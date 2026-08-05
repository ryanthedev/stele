# Preprocessor look-alikes

Everything here *resembles* something `decor::d2l`, `decor::quarto` or
`decor::codecogs` rewrites, and none of it may be touched. The suite asserts
that this whole file comes back byte-identical from all three passes, so a
pass that grows an appetite fails here rather than on a reader's document.

## Roles a reader is being shown, not using

The `:label:` role names a cross-reference target, and `:numref:` prints its
number. A ratio of 3:1 at 10:30 is neither.

```markdown
:label:`sec_linear_regression`
:eqlabel:`eq_price-area`
:width:`320px`

See :numref:`fig_single_neuron` and :cite:`Legendre.1805,Gauss.1809`.

:begin_tab:`mxnet`
Framework-specific prose.
:end_tab:
```

A fence that is not one of d2l's executable blocks keeps its info string:

```{.python}
x = 1
```

```{mermaid}
%%| echo: fenced
flowchart LR
  A --> B
```

```{=html}
<b>raw</b>
```

## Divs a reader is being shown

For example, here we add the "border" class to a region of content using a
div (`:::`):

``` markdown
::: {.border}
This content can be styled with a border
:::
```

Divs may also be nested, and the outer one takes more colons:

```` markdown
::::: {#special .sidebar}

::: {.warning}
Here is a warning.
:::

More content.
:::::
````

## Divs that are real but are not callouts

::: {.list-table header-rows=1 tbl-colwidths="[50, 50]"}
- - Markdown Syntax
  - Output

- - ``` markdown
    <https://quarto.org>
    ```
  - ::: pad-to-code-block
    <https://quarto.org>
    :::
:::

::: {.border .p-3}
A generic Pandoc div.
:::

::: {.warning}
A braced `.warning` is a generic div in Quarto's own documentation, not an
admonition.
:::

:::info
Docusaurus has kinds GFM has no counterpart for.
:::

## A grid table, whose columns are positional

+----------------------------------------+-------------------------------------------+
| Markdown Syntax                        | Output                                    |
+========================================+===========================================+
| ``` markdown                           | [This text is smallcaps]{.smallcaps}      |
| [This text is smallcaps]{.smallcaps}   |                                           |
| ```                                    |                                           |
+----------------------------------------+-------------------------------------------+
| ::: {.callout-note}                    | A note callout.                           |
| Body.                                  |                                           |
| :::                                    |                                           |
+----------------------------------------+-------------------------------------------+
| ![x](http://latex.codecogs.com/svg.latex?x)                                        ||
+----------------------------------------+-------------------------------------------+

## Brackets and braces that are not spans

A [link](https://example.com), a [reference][ref], a footnote[^1], and a
paragraph that ends in {a brace}.

You can include videos using the `{{{< video >}}}` shortcode.

## Images that are not equations

![a photograph](./img/cat.png) and a [link to codecogs](http://latex.codecogs.com/svg.latex?x).

[ref]: https://example.com
[^1]: A footnote.
