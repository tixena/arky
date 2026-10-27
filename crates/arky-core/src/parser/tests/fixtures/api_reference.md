# API reference

## Parsing

### `parse`

#### Arguments

##### `source`

###### Encoding

The source is UTF-8.

#### Returns

A `Document`, or a `ParseError`.

#### Example

```rust
let document = parse("# Title\n")?;
```

## Errors

An error carries a message and a byte range:

```json
{
  "message": "`tomorrow` is not a value",
  "span": { "start": 10, "end": 18 }
}
```

To show a fence inside a fence, use a longer one:

````markdown
```rust
fn main() {}
```
````
