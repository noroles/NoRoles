# Credentials

Every key an executor might use, and which permissions it can exercise.
A key that exercises nothing is open: `noroles run` hands it to the mandates that list it.
A key that exercises a permission is never handed out. It is used once, by `noroles do`, for an action someone approved.

Values live in `.noroles/secrets.env`, which never leaves this computer.

```yaml
credentials:
  github_read:  { env: GITHUB_TOKEN, exercises: [] }
  # stripe:     { env: STRIPE_KEY, exercises: [money.spend, price.change] }
```
