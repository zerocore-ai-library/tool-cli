"""
MCP server providing access to free public JSON APIs.

Categories:
- Weather: Open-Meteo, NWS
- Geo: Geocoding, countries, IP location
- Knowledge: Wikipedia, dictionary, books
- Finance: Crypto, currency exchange
- News: Hacker News, Reddit
- Entertainment: Movies, TV, trivia, Pokemon, games
- Science: NASA, SpaceX, earthquakes
- Food: Recipes, cocktails, nutrition
- Utilities: Random users, quotes, UUID
"""

import logging
from typing import Optional
from datetime import datetime, timedelta

import httpx
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("OpenData")
logging.disable(logging.INFO)

# Shared HTTP client settings
CLIENT_TIMEOUT = 30.0
USER_AGENT = "OpenDataMCP/1.0 (https://github.com/tool-cli; contact@example.com)"
DEFAULT_HEADERS = {"User-Agent": USER_AGENT}


async def fetch_json(
    url: str,
    params: Optional[dict] = None,
    headers: Optional[dict] = None,
) -> dict | list | str:
    """Fetch JSON from URL with error handling."""
    req_headers = {**DEFAULT_HEADERS, **(headers or {})}
    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT, headers=req_headers) as client:
        try:
            resp = await client.get(url, params=params)
            resp.raise_for_status()
            return resp.json()
        except httpx.HTTPStatusError as e:
            return {"error": f"HTTP {e.response.status_code}: {e.response.text[:200]}"}
        except httpx.RequestError as e:
            return {"error": f"Request failed: {str(e)}"}
        except Exception as e:
            return {"error": str(e)}


# =============================================================================
# WEATHER
# =============================================================================


@mcp.tool()
async def get_current_weather(latitude: float, longitude: float) -> dict:
    """
    Get current weather for a location.

    Args:
        latitude: Latitude (-90 to 90)
        longitude: Longitude (-180 to 180)

    Returns:
        Current temperature, humidity, wind, and conditions
    """
    url = "https://api.open-meteo.com/v1/forecast"
    params = {
        "latitude": latitude,
        "longitude": longitude,
        "current": "temperature_2m,relative_humidity_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m,wind_direction_10m",
        "temperature_unit": "celsius",
    }
    data = await fetch_json(url, params)
    if "error" in data:
        return data

    current = data.get("current", {})
    return {
        "location": {"latitude": latitude, "longitude": longitude},
        "temperature_c": current.get("temperature_2m"),
        "feels_like_c": current.get("apparent_temperature"),
        "humidity_percent": current.get("relative_humidity_2m"),
        "precipitation_mm": current.get("precipitation"),
        "wind_speed_kmh": current.get("wind_speed_10m"),
        "wind_direction_deg": current.get("wind_direction_10m"),
        "weather_code": current.get("weather_code"),
        "time": current.get("time"),
    }


@mcp.tool()
async def get_weather_forecast(
    latitude: float, longitude: float, days: int = 7
) -> dict:
    """
    Get weather forecast for a location.

    Args:
        latitude: Latitude (-90 to 90)
        longitude: Longitude (-180 to 180)
        days: Number of forecast days (1-16, default 7)

    Returns:
        Daily forecast with high/low temps, precipitation, and conditions
    """
    days = max(1, min(16, days))
    url = "https://api.open-meteo.com/v1/forecast"
    params = {
        "latitude": latitude,
        "longitude": longitude,
        "daily": "temperature_2m_max,temperature_2m_min,precipitation_sum,weather_code,wind_speed_10m_max",
        "temperature_unit": "celsius",
        "forecast_days": days,
    }
    data = await fetch_json(url, params)
    if "error" in data:
        return data

    daily = data.get("daily", {})
    forecasts = []
    dates = daily.get("time", [])
    for i, date in enumerate(dates):
        forecasts.append(
            {
                "date": date,
                "temp_max_c": daily.get("temperature_2m_max", [None])[i],
                "temp_min_c": daily.get("temperature_2m_min", [None])[i],
                "precipitation_mm": daily.get("precipitation_sum", [None])[i],
                "wind_max_kmh": daily.get("wind_speed_10m_max", [None])[i],
                "weather_code": daily.get("weather_code", [None])[i],
            }
        )

    return {"location": {"latitude": latitude, "longitude": longitude}, "forecast": forecasts}


@mcp.tool()
async def get_historical_weather(
    latitude: float, longitude: float, date: str
) -> dict:
    """
    Get historical weather for a specific date.

    Args:
        latitude: Latitude (-90 to 90)
        longitude: Longitude (-180 to 180)
        date: Date in YYYY-MM-DD format (must be in the past)

    Returns:
        Historical weather data for that date
    """
    url = "https://archive-api.open-meteo.com/v1/archive"
    params = {
        "latitude": latitude,
        "longitude": longitude,
        "start_date": date,
        "end_date": date,
        "daily": "temperature_2m_max,temperature_2m_min,precipitation_sum,wind_speed_10m_max",
        "temperature_unit": "celsius",
    }
    data = await fetch_json(url, params)
    if "error" in data:
        return data

    daily = data.get("daily", {})
    return {
        "location": {"latitude": latitude, "longitude": longitude},
        "date": date,
        "temp_max_c": daily.get("temperature_2m_max", [None])[0],
        "temp_min_c": daily.get("temperature_2m_min", [None])[0],
        "precipitation_mm": daily.get("precipitation_sum", [None])[0],
        "wind_max_kmh": daily.get("wind_speed_10m_max", [None])[0],
    }


# =============================================================================
# GEOGRAPHIC
# =============================================================================


@mcp.tool()
async def geocode(address: str) -> dict:
    """
    Convert an address or place name to coordinates.

    Args:
        address: Address or place name to geocode

    Returns:
        Latitude, longitude, and display name
    """
    url = "https://nominatim.openstreetmap.org/search"
    params = {"q": address, "format": "json", "limit": 1}
    headers = {"User-Agent": "OpenDataMCP/1.0"}

    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT) as client:
        try:
            resp = await client.get(url, params=params, headers=headers)
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            return {"error": str(e)}

    if not data:
        return {"error": "No results found"}

    result = data[0]
    return {
        "latitude": float(result["lat"]),
        "longitude": float(result["lon"]),
        "display_name": result.get("display_name"),
        "type": result.get("type"),
    }


@mcp.tool()
async def reverse_geocode(latitude: float, longitude: float) -> dict:
    """
    Convert coordinates to an address.

    Args:
        latitude: Latitude (-90 to 90)
        longitude: Longitude (-180 to 180)

    Returns:
        Address details for the location
    """
    url = "https://nominatim.openstreetmap.org/reverse"
    params = {"lat": latitude, "lon": longitude, "format": "json"}
    headers = {"User-Agent": "OpenDataMCP/1.0"}

    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT) as client:
        try:
            resp = await client.get(url, params=params, headers=headers)
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            return {"error": str(e)}

    if "error" in data:
        return {"error": data["error"]}

    addr = data.get("address", {})
    return {
        "display_name": data.get("display_name"),
        "country": addr.get("country"),
        "state": addr.get("state"),
        "city": addr.get("city") or addr.get("town") or addr.get("village"),
        "postcode": addr.get("postcode"),
    }


@mcp.tool()
async def get_country(name_or_code: str) -> dict:
    """
    Get detailed information about a country.

    Args:
        name_or_code: Country name or ISO code (e.g., "France" or "FR")

    Returns:
        Country details including population, currencies, languages
    """
    # Try by code first, then by name
    url = f"https://restcountries.com/v3.1/alpha/{name_or_code}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        url = f"https://restcountries.com/v3.1/name/{name_or_code}"
        data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data
    if not data:
        return {"error": "Country not found"}

    country = data[0] if isinstance(data, list) else data
    currencies = country.get("currencies", {})
    currency_list = [
        {"code": k, "name": v.get("name"), "symbol": v.get("symbol")}
        for k, v in currencies.items()
    ]

    return {
        "name": country.get("name", {}).get("common"),
        "official_name": country.get("name", {}).get("official"),
        "capital": country.get("capital", [None])[0],
        "region": country.get("region"),
        "subregion": country.get("subregion"),
        "population": country.get("population"),
        "area_km2": country.get("area"),
        "currencies": currency_list,
        "languages": list(country.get("languages", {}).values()),
        "timezones": country.get("timezones"),
        "flag_emoji": country.get("flag"),
        "maps": country.get("maps", {}).get("googleMaps"),
    }


@mcp.tool()
async def list_countries(region: Optional[str] = None) -> dict:
    """
    List all countries, optionally filtered by region.

    Args:
        region: Optional region filter (Africa, Americas, Asia, Europe, Oceania)

    Returns:
        List of countries with basic info
    """
    if region:
        url = f"https://restcountries.com/v3.1/region/{region}"
    else:
        url = "https://restcountries.com/v3.1/all"

    params = {"fields": "name,cca2,capital,population,region"}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    countries = []
    for c in data:
        countries.append(
            {
                "name": c.get("name", {}).get("common"),
                "code": c.get("cca2"),
                "capital": c.get("capital", [None])[0],
                "population": c.get("population"),
                "region": c.get("region"),
            }
        )

    # Sort by name
    countries.sort(key=lambda x: x["name"] or "")
    return {"count": len(countries), "countries": countries}


@mcp.tool()
async def get_ip_location(ip: Optional[str] = None) -> dict:
    """
    Get geolocation for an IP address.

    Args:
        ip: IP address to lookup (omit for your current IP)

    Returns:
        Location details for the IP
    """
    url = f"http://ip-api.com/json/{ip or ''}"
    data = await fetch_json(url)

    if isinstance(data, dict) and data.get("status") == "fail":
        return {"error": data.get("message", "Lookup failed")}

    return {
        "ip": data.get("query"),
        "country": data.get("country"),
        "country_code": data.get("countryCode"),
        "region": data.get("regionName"),
        "city": data.get("city"),
        "zip": data.get("zip"),
        "latitude": data.get("lat"),
        "longitude": data.get("lon"),
        "timezone": data.get("timezone"),
        "isp": data.get("isp"),
    }


# =============================================================================
# KNOWLEDGE
# =============================================================================


@mcp.tool()
async def wiki_summary(topic: str) -> dict:
    """
    Get a Wikipedia summary for a topic.

    Args:
        topic: Topic to look up

    Returns:
        Summary text and article URL
    """
    url = f"https://en.wikipedia.org/api/rest_v1/page/summary/{topic}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    return {
        "title": data.get("title"),
        "description": data.get("description"),
        "extract": data.get("extract"),
        "url": data.get("content_urls", {}).get("desktop", {}).get("page"),
    }


@mcp.tool()
async def wiki_search(query: str, limit: int = 10) -> dict:
    """
    Search Wikipedia for articles.

    Args:
        query: Search query
        limit: Max results (1-50, default 10)

    Returns:
        List of matching articles
    """
    limit = max(1, min(50, limit))
    url = "https://en.wikipedia.org/w/api.php"
    params = {
        "action": "query",
        "list": "search",
        "srsearch": query,
        "srlimit": limit,
        "format": "json",
    }
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    results = data.get("query", {}).get("search", [])
    articles = []
    for r in results:
        articles.append(
            {
                "title": r.get("title"),
                "snippet": r.get("snippet", "").replace("<span class=\"searchmatch\">", "").replace("</span>", ""),
                "word_count": r.get("wordcount"),
                "url": f"https://en.wikipedia.org/wiki/{r.get('title', '').replace(' ', '_')}",
            }
        )

    return {"query": query, "count": len(articles), "results": articles}


@mcp.tool()
async def define_word(word: str) -> dict:
    """
    Get dictionary definition of a word.

    Args:
        word: Word to define

    Returns:
        Definitions, phonetics, and examples
    """
    url = f"https://api.dictionaryapi.dev/api/v2/entries/en/{word}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data
    if isinstance(data, dict) and "title" in data:
        return {"error": data.get("message", "Word not found")}

    entry = data[0] if isinstance(data, list) else data
    phonetics = entry.get("phonetics", [])
    phonetic = next((p.get("text") for p in phonetics if p.get("text")), None)

    meanings = []
    for meaning in entry.get("meanings", []):
        defs = []
        for d in meaning.get("definitions", [])[:3]:
            defs.append(
                {
                    "definition": d.get("definition"),
                    "example": d.get("example"),
                }
            )
        meanings.append(
            {
                "part_of_speech": meaning.get("partOfSpeech"),
                "definitions": defs,
                "synonyms": meaning.get("synonyms", [])[:5],
            }
        )

    return {"word": word, "phonetic": phonetic, "meanings": meanings}


@mcp.tool()
async def get_book(query: str) -> dict:
    """
    Get book information by ISBN or title.

    Args:
        query: ISBN or book title

    Returns:
        Book metadata including authors, subjects, cover
    """
    # Try ISBN first
    url = f"https://openlibrary.org/isbn/{query}.json"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        # Try search
        search_url = "https://openlibrary.org/search.json"
        search_data = await fetch_json(search_url, {"q": query, "limit": 1})
        if isinstance(search_data, dict) and search_data.get("docs"):
            doc = search_data["docs"][0]
            return {
                "title": doc.get("title"),
                "authors": doc.get("author_name", []),
                "first_publish_year": doc.get("first_publish_year"),
                "subjects": doc.get("subject", [])[:10],
                "isbn": doc.get("isbn", [None])[0],
                "publishers": doc.get("publisher", [])[:3],
                "languages": doc.get("language", []),
                "cover_url": f"https://covers.openlibrary.org/b/id/{doc.get('cover_i')}-M.jpg"
                if doc.get("cover_i")
                else None,
            }
        return {"error": "Book not found"}

    # Get work details for more info
    work_key = data.get("works", [{}])[0].get("key", "")
    work_data = {}
    if work_key:
        work_data = await fetch_json(f"https://openlibrary.org{work_key}.json")

    return {
        "title": data.get("title"),
        "authors": [a.get("key", "").split("/")[-1] for a in data.get("authors", [])],
        "publish_date": data.get("publish_date"),
        "publishers": data.get("publishers", []),
        "pages": data.get("number_of_pages"),
        "subjects": work_data.get("subjects", [])[:10] if isinstance(work_data, dict) else [],
        "description": work_data.get("description", {}).get("value")
        if isinstance(work_data.get("description"), dict)
        else work_data.get("description"),
        "cover_url": f"https://covers.openlibrary.org/b/isbn/{query}-M.jpg",
    }


@mcp.tool()
async def search_books(query: str, limit: int = 10) -> dict:
    """
    Search for books by title, author, or subject.

    Args:
        query: Search query
        limit: Max results (1-100, default 10)

    Returns:
        List of matching books
    """
    limit = max(1, min(100, limit))
    url = "https://openlibrary.org/search.json"
    params = {"q": query, "limit": limit}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    books = []
    for doc in data.get("docs", []):
        books.append(
            {
                "title": doc.get("title"),
                "authors": doc.get("author_name", []),
                "first_publish_year": doc.get("first_publish_year"),
                "isbn": doc.get("isbn", [None])[0],
                "cover_url": f"https://covers.openlibrary.org/b/id/{doc.get('cover_i')}-S.jpg"
                if doc.get("cover_i")
                else None,
            }
        )

    return {"query": query, "count": len(books), "results": books}


# =============================================================================
# FINANCE
# =============================================================================


@mcp.tool()
async def get_crypto_price(coin: str) -> dict:
    """
    Get current price and market data for a cryptocurrency.

    Args:
        coin: Coin ID (e.g., "bitcoin", "ethereum", "dogecoin")

    Returns:
        Price, market cap, volume, and 24h change
    """
    url = f"https://api.coingecko.com/api/v3/coins/{coin.lower()}"
    params = {
        "localization": "false",
        "tickers": "false",
        "community_data": "false",
        "developer_data": "false",
    }
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    market = data.get("market_data", {})
    return {
        "id": data.get("id"),
        "symbol": data.get("symbol"),
        "name": data.get("name"),
        "price_usd": market.get("current_price", {}).get("usd"),
        "market_cap_usd": market.get("market_cap", {}).get("usd"),
        "volume_24h_usd": market.get("total_volume", {}).get("usd"),
        "change_24h_percent": market.get("price_change_percentage_24h"),
        "change_7d_percent": market.get("price_change_percentage_7d"),
        "ath_usd": market.get("ath", {}).get("usd"),
        "ath_date": market.get("ath_date", {}).get("usd"),
    }


@mcp.tool()
async def list_crypto(limit: int = 20) -> dict:
    """
    List top cryptocurrencies by market cap.

    Args:
        limit: Number of coins (1-100, default 20)

    Returns:
        List of top coins with prices and market data
    """
    limit = max(1, min(100, limit))
    url = "https://api.coingecko.com/api/v3/coins/markets"
    params = {
        "vs_currency": "usd",
        "order": "market_cap_desc",
        "per_page": limit,
        "page": 1,
    }
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    coins = []
    for c in data:
        coins.append(
            {
                "rank": c.get("market_cap_rank"),
                "id": c.get("id"),
                "symbol": c.get("symbol"),
                "name": c.get("name"),
                "price_usd": c.get("current_price"),
                "market_cap_usd": c.get("market_cap"),
                "change_24h_percent": c.get("price_change_percentage_24h"),
            }
        )

    return {"count": len(coins), "coins": coins}


@mcp.tool()
async def convert_currency(amount: float, from_currency: str, to_currency: str) -> dict:
    """
    Convert between currencies.

    Args:
        amount: Amount to convert
        from_currency: Source currency code (e.g., "USD")
        to_currency: Target currency code (e.g., "EUR")

    Returns:
        Converted amount and exchange rate
    """
    from_currency = from_currency.upper()
    to_currency = to_currency.upper()

    url = f"https://api.exchangerate-api.com/v4/latest/{from_currency}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    rates = data.get("rates", {})
    if to_currency not in rates:
        return {"error": f"Unknown currency: {to_currency}"}

    rate = rates[to_currency]
    return {
        "from": {"currency": from_currency, "amount": amount},
        "to": {"currency": to_currency, "amount": round(amount * rate, 4)},
        "rate": rate,
        "date": data.get("date"),
    }


@mcp.tool()
async def get_exchange_rates(base: str = "USD") -> dict:
    """
    Get exchange rates for a base currency.

    Args:
        base: Base currency code (default USD)

    Returns:
        Exchange rates for all currencies
    """
    base = base.upper()
    url = f"https://api.exchangerate-api.com/v4/latest/{base}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    return {
        "base": data.get("base"),
        "date": data.get("date"),
        "rates": data.get("rates"),
    }


# =============================================================================
# NEWS & SOCIAL
# =============================================================================


@mcp.tool()
async def hn_top_stories(limit: int = 20) -> dict:
    """
    Get top stories from Hacker News.

    Args:
        limit: Number of stories (1-100, default 20)

    Returns:
        List of top stories with titles, URLs, and scores
    """
    limit = max(1, min(100, limit))

    # Get top story IDs
    url = "https://hacker-news.firebaseio.com/v0/topstories.json"
    ids = await fetch_json(url)

    if isinstance(ids, dict) and "error" in ids:
        return ids

    # Fetch story details (limit concurrent requests)
    stories = []
    for story_id in ids[:limit]:
        story_url = f"https://hacker-news.firebaseio.com/v0/item/{story_id}.json"
        story = await fetch_json(story_url)
        if isinstance(story, dict) and "error" not in story:
            stories.append(
                {
                    "id": story.get("id"),
                    "title": story.get("title"),
                    "url": story.get("url"),
                    "score": story.get("score"),
                    "by": story.get("by"),
                    "comments": story.get("descendants", 0),
                    "hn_url": f"https://news.ycombinator.com/item?id={story.get('id')}",
                }
            )

    return {"count": len(stories), "stories": stories}


@mcp.tool()
async def hn_story(story_id: int) -> dict:
    """
    Get a Hacker News story with top comments.

    Args:
        story_id: HN story ID

    Returns:
        Story details and top comments
    """
    url = f"https://hacker-news.firebaseio.com/v0/item/{story_id}.json"
    story = await fetch_json(url)

    if isinstance(story, dict) and "error" in story:
        return story

    # Get top comments
    comments = []
    for kid_id in story.get("kids", [])[:10]:
        comment_url = f"https://hacker-news.firebaseio.com/v0/item/{kid_id}.json"
        comment = await fetch_json(comment_url)
        if isinstance(comment, dict) and comment.get("text"):
            comments.append(
                {
                    "id": comment.get("id"),
                    "by": comment.get("by"),
                    "text": comment.get("text")[:500],
                }
            )

    return {
        "id": story.get("id"),
        "title": story.get("title"),
        "url": story.get("url"),
        "text": story.get("text"),
        "score": story.get("score"),
        "by": story.get("by"),
        "time": story.get("time"),
        "comment_count": story.get("descendants", 0),
        "top_comments": comments,
    }


@mcp.tool()
async def reddit_posts(subreddit: str, sort: str = "hot", limit: int = 20) -> dict:
    """
    Get posts from a subreddit.

    Args:
        subreddit: Subreddit name (without r/)
        sort: Sort order (hot, new, top, rising)
        limit: Number of posts (1-100, default 20)

    Returns:
        List of posts with titles, scores, and URLs
    """
    limit = max(1, min(100, limit))
    sort = sort.lower() if sort.lower() in ("hot", "new", "top", "rising") else "hot"

    url = f"https://old.reddit.com/r/{subreddit}/{sort}.json"
    params = {"limit": limit, "raw_json": 1}

    # Reddit requires a browser-like User-Agent
    headers = {
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Accept": "application/json",
    }

    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT, follow_redirects=True) as client:
        try:
            resp = await client.get(url, params=params, headers=headers)
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            return {"error": str(e)}

    posts = []
    for child in data.get("data", {}).get("children", []):
        post = child.get("data", {})
        posts.append(
            {
                "id": post.get("id"),
                "title": post.get("title"),
                "author": post.get("author"),
                "score": post.get("score"),
                "upvote_ratio": post.get("upvote_ratio"),
                "comments": post.get("num_comments"),
                "url": post.get("url"),
                "permalink": f"https://reddit.com{post.get('permalink')}",
                "selftext": post.get("selftext", "")[:300] if post.get("selftext") else None,
            }
        )

    return {"subreddit": subreddit, "sort": sort, "count": len(posts), "posts": posts}


@mcp.tool()
async def reddit_post(subreddit: str, post_id: str) -> dict:
    """
    Get a Reddit post with top comments.

    Args:
        subreddit: Subreddit name
        post_id: Post ID

    Returns:
        Post details and top comments
    """
    url = f"https://old.reddit.com/r/{subreddit}/comments/{post_id}.json"
    params = {"raw_json": 1}

    headers = {
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Accept": "application/json",
    }

    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT, follow_redirects=True) as client:
        try:
            resp = await client.get(url, params=params, headers=headers)
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            return {"error": str(e)}

    if not data or len(data) < 2:
        return {"error": "Post not found"}

    post = data[0].get("data", {}).get("children", [{}])[0].get("data", {})
    comment_children = data[1].get("data", {}).get("children", [])

    comments = []
    for child in comment_children[:10]:
        c = child.get("data", {})
        if c.get("body"):
            comments.append(
                {
                    "author": c.get("author"),
                    "score": c.get("score"),
                    "body": c.get("body", "")[:500],
                }
            )

    return {
        "id": post.get("id"),
        "title": post.get("title"),
        "author": post.get("author"),
        "score": post.get("score"),
        "upvote_ratio": post.get("upvote_ratio"),
        "selftext": post.get("selftext"),
        "url": post.get("url"),
        "created_utc": post.get("created_utc"),
        "comment_count": post.get("num_comments"),
        "top_comments": comments,
    }


# =============================================================================
# ENTERTAINMENT
# =============================================================================


@mcp.tool()
async def search_movies(query: str) -> dict:
    """
    Search for movies by title.

    Args:
        query: Movie title to search

    Returns:
        List of matching movies
    """
    url = "https://api.tvmaze.com/search/shows"
    params = {"q": query}
    data = await fetch_json(url, params)

    # TVMaze is for TV, use OMDb for movies but it needs API key
    # Fall back to a free alternative
    url = f"https://www.omdbapi.com/?apikey=trilogy&s={query}"
    data = await fetch_json(url)

    if isinstance(data, dict) and data.get("Response") == "False":
        return {"error": data.get("Error", "No results")}

    movies = []
    for m in data.get("Search", []):
        movies.append(
            {
                "imdb_id": m.get("imdbID"),
                "title": m.get("Title"),
                "year": m.get("Year"),
                "type": m.get("Type"),
                "poster": m.get("Poster") if m.get("Poster") != "N/A" else None,
            }
        )

    return {"query": query, "count": len(movies), "results": movies}


@mcp.tool()
async def get_movie(imdb_id: str) -> dict:
    """
    Get detailed movie information.

    Args:
        imdb_id: IMDB ID (e.g., "tt0111161")

    Returns:
        Full movie details including plot, ratings, cast
    """
    url = f"https://www.omdbapi.com/?apikey=trilogy&i={imdb_id}&plot=full"
    data = await fetch_json(url)

    if isinstance(data, dict) and data.get("Response") == "False":
        return {"error": data.get("Error", "Movie not found")}

    return {
        "imdb_id": data.get("imdbID"),
        "title": data.get("Title"),
        "year": data.get("Year"),
        "rated": data.get("Rated"),
        "released": data.get("Released"),
        "runtime": data.get("Runtime"),
        "genres": data.get("Genre", "").split(", "),
        "director": data.get("Director"),
        "writers": data.get("Writer", "").split(", "),
        "actors": data.get("Actors", "").split(", "),
        "plot": data.get("Plot"),
        "language": data.get("Language"),
        "country": data.get("Country"),
        "awards": data.get("Awards"),
        "imdb_rating": data.get("imdbRating"),
        "imdb_votes": data.get("imdbVotes"),
        "box_office": data.get("BoxOffice"),
    }


@mcp.tool()
async def search_tv(query: str) -> dict:
    """
    Search for TV shows.

    Args:
        query: TV show title to search

    Returns:
        List of matching shows
    """
    url = "https://api.tvmaze.com/search/shows"
    params = {"q": query}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    shows = []
    for item in data:
        show = item.get("show", {})
        shows.append(
            {
                "id": show.get("id"),
                "name": show.get("name"),
                "type": show.get("type"),
                "language": show.get("language"),
                "genres": show.get("genres", []),
                "status": show.get("status"),
                "premiered": show.get("premiered"),
                "rating": show.get("rating", {}).get("average"),
                "url": show.get("url"),
            }
        )

    return {"query": query, "count": len(shows), "results": shows}


@mcp.tool()
async def get_tv_show(show_id: int) -> dict:
    """
    Get detailed TV show information.

    Args:
        show_id: TVMaze show ID

    Returns:
        Full show details including seasons and episodes
    """
    url = f"https://api.tvmaze.com/shows/{show_id}"
    params = {"embed[]": ["episodes", "cast"]}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    episodes = data.get("_embedded", {}).get("episodes", [])
    cast = data.get("_embedded", {}).get("cast", [])

    # Count seasons
    seasons = {}
    for ep in episodes:
        s = ep.get("season")
        if s not in seasons:
            seasons[s] = 0
        seasons[s] += 1

    return {
        "id": data.get("id"),
        "name": data.get("name"),
        "type": data.get("type"),
        "language": data.get("language"),
        "genres": data.get("genres"),
        "status": data.get("status"),
        "premiered": data.get("premiered"),
        "ended": data.get("ended"),
        "runtime": data.get("runtime"),
        "rating": data.get("rating", {}).get("average"),
        "summary": data.get("summary", "").replace("<p>", "").replace("</p>", ""),
        "seasons": len(seasons),
        "total_episodes": len(episodes),
        "cast": [
            {"name": c.get("person", {}).get("name"), "character": c.get("character", {}).get("name")}
            for c in cast[:10]
        ],
        "url": data.get("url"),
    }


@mcp.tool()
async def get_trivia(
    amount: int = 10, category: Optional[int] = None, difficulty: Optional[str] = None
) -> dict:
    """
    Get trivia questions.

    Args:
        amount: Number of questions (1-50, default 10)
        category: Category ID (9=General, 10=Books, 11=Film, 12=Music, etc.)
        difficulty: easy, medium, or hard

    Returns:
        List of trivia questions with answers
    """
    amount = max(1, min(50, amount))
    url = "https://opentdb.com/api.php"
    params = {"amount": amount, "type": "multiple"}

    if category:
        params["category"] = category
    if difficulty and difficulty in ("easy", "medium", "hard"):
        params["difficulty"] = difficulty

    data = await fetch_json(url, params)

    if isinstance(data, dict) and data.get("response_code") != 0:
        return {"error": "Failed to fetch trivia questions"}

    questions = []
    for q in data.get("results", []):
        questions.append(
            {
                "category": q.get("category"),
                "difficulty": q.get("difficulty"),
                "question": q.get("question"),
                "correct_answer": q.get("correct_answer"),
                "incorrect_answers": q.get("incorrect_answers"),
            }
        )

    return {"count": len(questions), "questions": questions}


@mcp.tool()
async def get_pokemon(name_or_id: str) -> dict:
    """
    Get Pokemon information.

    Args:
        name_or_id: Pokemon name or Pokedex number

    Returns:
        Pokemon stats, types, and abilities
    """
    url = f"https://pokeapi.co/api/v2/pokemon/{str(name_or_id).lower()}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    stats = {s.get("stat", {}).get("name"): s.get("base_stat") for s in data.get("stats", [])}

    return {
        "id": data.get("id"),
        "name": data.get("name"),
        "height_dm": data.get("height"),
        "weight_hg": data.get("weight"),
        "types": [t.get("type", {}).get("name") for t in data.get("types", [])],
        "abilities": [a.get("ability", {}).get("name") for a in data.get("abilities", [])],
        "stats": stats,
        "base_experience": data.get("base_experience"),
    }


@mcp.tool()
async def search_games(query: str, limit: int = 10) -> dict:
    """
    Search for video games and find deals.

    Args:
        query: Game title to search
        limit: Max results (1-60, default 10)

    Returns:
        List of matching games with deal prices
    """
    limit = max(1, min(60, limit))
    # Use CheapShark API - free, no key required
    url = "https://www.cheapshark.com/api/1.0/games"
    params = {"title": query, "limit": limit}

    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    if not isinstance(data, list):
        return {"error": "Unexpected response format"}

    games = []
    for g in data:
        games.append(
            {
                "game_id": g.get("gameID"),
                "name": g.get("external"),
                "cheapest_price": g.get("cheapest"),
                "cheapest_deal_id": g.get("cheapestDealID"),
                "thumb": g.get("thumb"),
            }
        )

    return {"query": query, "count": len(games), "results": games}


# =============================================================================
# SCIENCE
# =============================================================================


@mcp.tool()
async def nasa_apod(date: Optional[str] = None) -> dict:
    """
    Get NASA's Astronomy Picture of the Day.

    Args:
        date: Date in YYYY-MM-DD format (default: today)

    Returns:
        APOD metadata including title, explanation, and image URL
    """
    url = "https://api.nasa.gov/planetary/apod"
    params = {"api_key": "DEMO_KEY"}
    if date:
        params["date"] = date

    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    return {
        "title": data.get("title"),
        "date": data.get("date"),
        "explanation": data.get("explanation"),
        "media_type": data.get("media_type"),
        "url": data.get("url"),
        "hdurl": data.get("hdurl"),
        "copyright": data.get("copyright"),
    }


@mcp.tool()
async def get_asteroids(start_date: str, end_date: str) -> dict:
    """
    Get near-Earth asteroids for a date range.

    Args:
        start_date: Start date (YYYY-MM-DD)
        end_date: End date (YYYY-MM-DD, max 7 days from start)

    Returns:
        List of near-Earth objects with size and approach data
    """
    url = "https://api.nasa.gov/neo/rest/v1/feed"
    params = {"start_date": start_date, "end_date": end_date, "api_key": "DEMO_KEY"}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    asteroids = []
    for date, neos in data.get("near_earth_objects", {}).items():
        for neo in neos:
            approach = neo.get("close_approach_data", [{}])[0]
            asteroids.append(
                {
                    "id": neo.get("id"),
                    "name": neo.get("name"),
                    "diameter_km_min": neo.get("estimated_diameter", {})
                    .get("kilometers", {})
                    .get("estimated_diameter_min"),
                    "diameter_km_max": neo.get("estimated_diameter", {})
                    .get("kilometers", {})
                    .get("estimated_diameter_max"),
                    "is_potentially_hazardous": neo.get("is_potentially_hazardous_asteroid"),
                    "close_approach_date": approach.get("close_approach_date"),
                    "miss_distance_km": approach.get("miss_distance", {}).get("kilometers"),
                    "velocity_kmh": approach.get("relative_velocity", {}).get("kilometers_per_hour"),
                }
            )

    return {
        "start_date": start_date,
        "end_date": end_date,
        "count": data.get("element_count"),
        "asteroids": asteroids,
    }


@mcp.tool()
async def spacex_launches(upcoming: bool = True, limit: int = 10) -> dict:
    """
    Get SpaceX launches.

    Args:
        upcoming: True for upcoming, False for past launches
        limit: Number of launches (1-100, default 10)

    Returns:
        List of launches with dates, rockets, and payloads
    """
    limit = max(1, min(100, limit))
    url = "https://api.spacexdata.com/v5/launches/query"

    query = {"upcoming": upcoming}
    options = {"limit": limit, "sort": {"date_utc": 1 if upcoming else -1}}

    async with httpx.AsyncClient(timeout=CLIENT_TIMEOUT) as client:
        try:
            resp = await client.post(url, json={"query": query, "options": options})
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            return {"error": str(e)}

    launches = []
    for launch in data.get("docs", []):
        launches.append(
            {
                "id": launch.get("id"),
                "name": launch.get("name"),
                "date_utc": launch.get("date_utc"),
                "rocket": launch.get("rocket"),
                "success": launch.get("success"),
                "details": launch.get("details"),
                "webcast": launch.get("links", {}).get("webcast"),
            }
        )

    return {"upcoming": upcoming, "count": len(launches), "launches": launches}


@mcp.tool()
async def spacex_launch(launch_id: str) -> dict:
    """
    Get details for a specific SpaceX launch.

    Args:
        launch_id: Launch ID

    Returns:
        Full launch details
    """
    url = f"https://api.spacexdata.com/v5/launches/{launch_id}"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    return {
        "id": data.get("id"),
        "name": data.get("name"),
        "date_utc": data.get("date_utc"),
        "rocket": data.get("rocket"),
        "success": data.get("success"),
        "failures": data.get("failures"),
        "details": data.get("details"),
        "crew": data.get("crew"),
        "ships": data.get("ships"),
        "payloads": data.get("payloads"),
        "launchpad": data.get("launchpad"),
        "links": data.get("links"),
    }


@mcp.tool()
async def get_earthquakes(
    min_magnitude: float = 4.5, days: int = 7, limit: int = 20
) -> dict:
    """
    Get recent earthquakes.

    Args:
        min_magnitude: Minimum magnitude (default 4.5)
        days: Look back period in days (1-30, default 7)
        limit: Max results (1-100, default 20)

    Returns:
        List of earthquakes with location, magnitude, and time
    """
    days = max(1, min(30, days))
    limit = max(1, min(100, limit))

    end = datetime.utcnow()
    start = end - timedelta(days=days)

    url = "https://earthquake.usgs.gov/fdsnws/event/1/query"
    params = {
        "format": "geojson",
        "starttime": start.strftime("%Y-%m-%d"),
        "endtime": end.strftime("%Y-%m-%d"),
        "minmagnitude": min_magnitude,
        "limit": limit,
        "orderby": "time",
    }
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    quakes = []
    for feature in data.get("features", []):
        props = feature.get("properties", {})
        coords = feature.get("geometry", {}).get("coordinates", [])
        quakes.append(
            {
                "id": feature.get("id"),
                "magnitude": props.get("mag"),
                "place": props.get("place"),
                "time": props.get("time"),
                "latitude": coords[1] if len(coords) > 1 else None,
                "longitude": coords[0] if coords else None,
                "depth_km": coords[2] if len(coords) > 2 else None,
                "url": props.get("url"),
            }
        )

    return {
        "min_magnitude": min_magnitude,
        "days": days,
        "count": len(quakes),
        "earthquakes": quakes,
    }


# =============================================================================
# FOOD
# =============================================================================


@mcp.tool()
async def search_recipes(query: str) -> dict:
    """
    Search for meal recipes.

    Args:
        query: Recipe name or ingredient to search

    Returns:
        List of matching recipes
    """
    url = "https://www.themealdb.com/api/json/v1/1/search.php"
    params = {"s": query}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    meals = data.get("meals") or []
    recipes = []
    for m in meals:
        recipes.append(
            {
                "id": m.get("idMeal"),
                "name": m.get("strMeal"),
                "category": m.get("strCategory"),
                "area": m.get("strArea"),
                "thumbnail": m.get("strMealThumb"),
            }
        )

    return {"query": query, "count": len(recipes), "results": recipes}


@mcp.tool()
async def get_recipe(meal_id: str) -> dict:
    """
    Get full recipe details.

    Args:
        meal_id: TheMealDB meal ID

    Returns:
        Full recipe with ingredients and instructions
    """
    url = "https://www.themealdb.com/api/json/v1/1/lookup.php"
    params = {"i": meal_id}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    meals = data.get("meals")
    if not meals:
        return {"error": "Recipe not found"}

    m = meals[0]

    # Extract ingredients
    ingredients = []
    for i in range(1, 21):
        ing = m.get(f"strIngredient{i}")
        measure = m.get(f"strMeasure{i}")
        if ing and ing.strip():
            ingredients.append({"ingredient": ing, "measure": measure})

    return {
        "id": m.get("idMeal"),
        "name": m.get("strMeal"),
        "category": m.get("strCategory"),
        "area": m.get("strArea"),
        "instructions": m.get("strInstructions"),
        "ingredients": ingredients,
        "youtube": m.get("strYoutube"),
        "source": m.get("strSource"),
        "thumbnail": m.get("strMealThumb"),
    }


@mcp.tool()
async def random_recipe() -> dict:
    """
    Get a random recipe.

    Returns:
        Random recipe with full details
    """
    url = "https://www.themealdb.com/api/json/v1/1/random.php"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    meals = data.get("meals")
    if not meals:
        return {"error": "No recipe found"}

    m = meals[0]

    ingredients = []
    for i in range(1, 21):
        ing = m.get(f"strIngredient{i}")
        measure = m.get(f"strMeasure{i}")
        if ing and ing.strip():
            ingredients.append({"ingredient": ing, "measure": measure})

    return {
        "id": m.get("idMeal"),
        "name": m.get("strMeal"),
        "category": m.get("strCategory"),
        "area": m.get("strArea"),
        "instructions": m.get("strInstructions"),
        "ingredients": ingredients,
        "thumbnail": m.get("strMealThumb"),
    }


@mcp.tool()
async def search_cocktails(query: str) -> dict:
    """
    Search for cocktail recipes.

    Args:
        query: Cocktail name to search

    Returns:
        List of matching cocktails
    """
    url = "https://www.thecocktaildb.com/api/json/v1/1/search.php"
    params = {"s": query}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    drinks = data.get("drinks") or []
    cocktails = []
    for d in drinks:
        cocktails.append(
            {
                "id": d.get("idDrink"),
                "name": d.get("strDrink"),
                "category": d.get("strCategory"),
                "glass": d.get("strGlass"),
                "alcoholic": d.get("strAlcoholic"),
                "thumbnail": d.get("strDrinkThumb"),
            }
        )

    return {"query": query, "count": len(cocktails), "results": cocktails}


@mcp.tool()
async def get_cocktail(drink_id: str) -> dict:
    """
    Get full cocktail recipe.

    Args:
        drink_id: TheCocktailDB drink ID

    Returns:
        Full recipe with ingredients and instructions
    """
    url = "https://www.thecocktaildb.com/api/json/v1/1/lookup.php"
    params = {"i": drink_id}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    drinks = data.get("drinks")
    if not drinks:
        return {"error": "Cocktail not found"}

    d = drinks[0]

    ingredients = []
    for i in range(1, 16):
        ing = d.get(f"strIngredient{i}")
        measure = d.get(f"strMeasure{i}")
        if ing and ing.strip():
            ingredients.append({"ingredient": ing, "measure": measure})

    return {
        "id": d.get("idDrink"),
        "name": d.get("strDrink"),
        "category": d.get("strCategory"),
        "glass": d.get("strGlass"),
        "alcoholic": d.get("strAlcoholic"),
        "instructions": d.get("strInstructions"),
        "ingredients": ingredients,
        "thumbnail": d.get("strDrinkThumb"),
    }


@mcp.tool()
async def get_product_nutrition(barcode: str) -> dict:
    """
    Get nutrition facts for a product by barcode.

    Args:
        barcode: Product barcode (UPC/EAN)

    Returns:
        Product name and nutritional information
    """
    url = f"https://world.openfoodfacts.org/api/v0/product/{barcode}.json"
    data = await fetch_json(url)

    if isinstance(data, dict) and "error" in data:
        return data

    if data.get("status") == 0:
        return {"error": "Product not found"}

    product = data.get("product", {})
    nutrients = product.get("nutriments", {})

    return {
        "barcode": barcode,
        "name": product.get("product_name"),
        "brands": product.get("brands"),
        "categories": product.get("categories"),
        "serving_size": product.get("serving_size"),
        "nutrition_per_100g": {
            "energy_kcal": nutrients.get("energy-kcal_100g"),
            "fat_g": nutrients.get("fat_100g"),
            "saturated_fat_g": nutrients.get("saturated-fat_100g"),
            "carbs_g": nutrients.get("carbohydrates_100g"),
            "sugars_g": nutrients.get("sugars_100g"),
            "fiber_g": nutrients.get("fiber_100g"),
            "protein_g": nutrients.get("proteins_100g"),
            "salt_g": nutrients.get("salt_100g"),
        },
        "ingredients": product.get("ingredients_text"),
        "nutriscore": product.get("nutriscore_grade"),
    }


# =============================================================================
# UTILITIES
# =============================================================================


@mcp.tool()
async def random_user(count: int = 1) -> dict:
    """
    Generate random user profiles.

    Args:
        count: Number of users (1-100, default 1)

    Returns:
        List of fake user profiles
    """
    count = max(1, min(100, count))
    url = "https://randomuser.me/api/"
    params = {"results": count}
    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        return data

    users = []
    for u in data.get("results", []):
        users.append(
            {
                "name": f"{u.get('name', {}).get('first')} {u.get('name', {}).get('last')}",
                "email": u.get("email"),
                "username": u.get("login", {}).get("username"),
                "gender": u.get("gender"),
                "location": {
                    "city": u.get("location", {}).get("city"),
                    "state": u.get("location", {}).get("state"),
                    "country": u.get("location", {}).get("country"),
                },
                "phone": u.get("phone"),
                "dob": u.get("dob", {}).get("date"),
                "picture": u.get("picture", {}).get("medium"),
            }
        )

    return {"count": len(users), "users": users}


@mcp.tool()
async def random_quote(category: Optional[str] = None) -> dict:
    """
    Get a random quote.

    Args:
        category: Optional category (inspire, management, sports, life, funny, love, art, students)

    Returns:
        Random quote with author
    """
    url = "https://api.quotable.io/random"
    params = {}
    if category:
        params["tags"] = category

    data = await fetch_json(url, params)

    if isinstance(data, dict) and "error" in data:
        # Fallback to simpler API
        fallback_url = "https://zenquotes.io/api/random"
        fallback_data = await fetch_json(fallback_url)
        if isinstance(fallback_data, list) and fallback_data:
            q = fallback_data[0]
            return {"content": q.get("q"), "author": q.get("a")}
        return data

    return {
        "content": data.get("content"),
        "author": data.get("author"),
        "tags": data.get("tags"),
    }


@mcp.tool()
async def generate_uuid(count: int = 1) -> dict:
    """
    Generate random UUIDs.

    Args:
        count: Number of UUIDs (1-100, default 1)

    Returns:
        List of UUIDs
    """
    import uuid as uuid_lib

    count = max(1, min(100, count))
    uuids = [str(uuid_lib.uuid4()) for _ in range(count)]

    return {"count": len(uuids), "uuids": uuids}


if __name__ == "__main__":
    mcp.run(transport="stdio")
