# Open Data MCP Server

MCP server providing access to 42 free public JSON APIs.

## Quick Start

```bash
uv sync
uv run server.py
```

## Testing

```bash
uv sync --extra test
uv run test_server.py
```

Output:
```
Results: 42 passed, 0 failed / 42 total
```

## Tools (42 total)

### Weather (3)
| Tool | Description |
|------|-------------|
| `get_current_weather` | Current conditions by lat/lon |
| `get_weather_forecast` | Multi-day forecast (1-16 days) |
| `get_historical_weather` | Past weather for a date |

### Geographic (5)
| Tool | Description |
|------|-------------|
| `geocode` | Address → coordinates |
| `reverse_geocode` | Coordinates → address |
| `get_country` | Country details by name/code |
| `list_countries` | All countries (filter by region) |
| `get_ip_location` | IP geolocation |

### Knowledge (5)
| Tool | Description |
|------|-------------|
| `wiki_summary` | Wikipedia article summary |
| `wiki_search` | Search Wikipedia |
| `define_word` | Dictionary definitions |
| `get_book` | Book info by ISBN/title |
| `search_books` | Search Open Library |

### Finance (4)
| Tool | Description |
|------|-------------|
| `get_crypto_price` | Crypto price + market data |
| `list_crypto` | Top cryptocurrencies |
| `convert_currency` | Currency conversion |
| `get_exchange_rates` | All rates for a base currency |

### News & Social (4)
| Tool | Description |
|------|-------------|
| `hn_top_stories` | Hacker News top stories |
| `hn_story` | HN story with comments |
| `reddit_posts` | Subreddit posts |
| `reddit_post` | Reddit post with comments |

### Entertainment (7)
| Tool | Description |
|------|-------------|
| `search_movies` | Search movies (OMDb) |
| `get_movie` | Movie details by IMDB ID |
| `search_tv` | Search TV shows (TVMaze) |
| `get_tv_show` | TV show details |
| `get_trivia` | Trivia questions |
| `get_pokemon` | Pokemon stats (PokeAPI) |
| `search_games` | Video game search |

### Science (5)
| Tool | Description |
|------|-------------|
| `nasa_apod` | Astronomy Picture of the Day |
| `get_asteroids` | Near-Earth asteroids |
| `spacex_launches` | SpaceX launch list |
| `spacex_launch` | Launch details |
| `get_earthquakes` | Recent earthquakes (USGS) |

### Food (6)
| Tool | Description |
|------|-------------|
| `search_recipes` | Search meal recipes |
| `get_recipe` | Full recipe with ingredients |
| `random_recipe` | Random recipe |
| `search_cocktails` | Search cocktails |
| `get_cocktail` | Cocktail recipe |
| `get_product_nutrition` | Nutrition by barcode |

### Utilities (3)
| Tool | Description |
|------|-------------|
| `random_user` | Fake user profiles |
| `random_quote` | Random quote |
| `generate_uuid` | Generate UUIDs |

## API Sources

All free, no API keys required:

| Category | APIs |
|----------|------|
| Weather | Open-Meteo |
| Geo | Nominatim, REST Countries, IP-API |
| Knowledge | Wikipedia, DictionaryAPI, Open Library |
| Finance | CoinGecko, ExchangeRate-API |
| News | Hacker News, Reddit |
| Entertainment | OMDb, TVMaze, Open Trivia DB, PokeAPI, CheapShark |
| Science | NASA, SpaceX, USGS |
| Food | TheMealDB, TheCocktailDB, Open Food Facts |
| Utilities | RandomUser, Quotable |
