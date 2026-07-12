# Metadata Protocol

The public client must not contain the private oanor API key. A project-controlled metadata service will fetch upstream data, normalize it, assign catalogue revisions, and serve compact client updates.

## Client Request

```json
{
  "after_revision": 12,
  "client_schema_version": 1
}
```

## Service Response

```json
{
  "schema_version": 1,
  "from_revision": 12,
  "to_revision": 13,
  "generated_at": "2026-07-11T00:00:00Z",
  "cards": []
}
```

Each card record uses the normalized client schema:
- `key`
- `name`
- `cost`
- `power`
- `ability`
- `collectable`
- `released`
- `series`
- `image_url`
- `revision`

Updates must be validated and applied transactionally. Match tracking must not block on metadata synchronization.

## Image URLs

The service returns upstream card-art URLs. The client downloads art directly and caches by full URL. A changed URL is a new asset. The project server is not an image CDN by default.

Direct automated access to upstream image URLs remains a licensing and permission question.
